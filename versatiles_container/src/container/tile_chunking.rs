//! Shared chunking logic for coalescing nearby tile byte ranges into bulk reads.
//!
//! Both the VersaTiles and PMTiles readers use this module to minimize I/O calls
//! when streaming tiles for a bounding box. Nearby byte ranges are grouped into
//! chunks (up to ~64 MiB each, with a small gap tolerance), which are then read
//! as single large blobs and sliced into individual tiles.

use crate::Tile;
use futures::stream::StreamExt;
use std::sync::Arc;
use versatiles_core::{Blob, ByteRange, TileCompression, TileCoord, TileFormat, TileStream, io::DataReader};

const MAX_CHUNK_SIZE: u64 = 64 * 1024 * 1024;
const MAX_CHUNK_GAP: u64 = 256 * 1024;

/// A group of tile byte ranges that can be served from a single large read.
/// `range` tracks the combined byte span in the container.
#[derive(Debug)]
pub struct Chunk {
	pub tiles: Vec<(TileCoord, ByteRange)>,
	pub range: ByteRange,
}

impl Chunk {
	pub fn new(start: u64) -> Self {
		Self {
			tiles: Vec::new(),
			range: ByteRange::new(start, 0),
		}
	}

	pub fn push(&mut self, entry: (TileCoord, ByteRange)) {
		assert!(
			entry.1.offset >= self.range.offset,
			"entry offset must be >= range offset"
		);
		self.range.length = self
			.range
			.length
			.max(entry.1.offset + entry.1.length - self.range.offset);
		self.tiles.push(entry);
	}
}

/// Sort tile ranges by byte offset and coalesce into chunks.
///
/// Nearby ranges (within `MAX_CHUNK_GAP`) are grouped together as long as the
/// total chunk size stays below `MAX_CHUNK_SIZE`.
pub fn coalesce_into_chunks(mut tile_ranges: Vec<(TileCoord, ByteRange)>) -> Vec<Chunk> {
	if tile_ranges.is_empty() {
		return Vec::new();
	}

	tile_ranges.sort_by_key(|e| e.1.offset);

	let mut chunks: Vec<Chunk> = Vec::new();
	let mut chunk = Chunk::new(tile_ranges[0].1.offset);

	for entry in tile_ranges {
		let chunk_start = chunk.range.offset;
		let chunk_end = chunk.range.offset + chunk.range.length;

		let tile_start = entry.1.offset;
		let tile_end = entry.1.offset + entry.1.length;

		if (chunk_start + MAX_CHUNK_SIZE > tile_end) && (chunk_end + MAX_CHUNK_GAP > tile_start) {
			chunk.push(entry);
		} else {
			chunks.push(chunk);
			chunk = Chunk::new(entry.1.offset);
			chunk.push(entry);
		}
	}

	if !chunk.tiles.is_empty() {
		chunks.push(chunk);
	}

	chunks
}

/// Convert chunks into a `TileStream` by reading each chunk as a single blob
/// and slicing out individual tiles.
pub fn stream_from_chunks(
	chunks: Vec<Chunk>,
	reader: Arc<DataReader>,
	tile_compression: TileCompression,
	tile_format: TileFormat,
) -> TileStream<'static, Tile> {
	TileStream::from_stream(
		futures::stream::iter(chunks)
			.then(move |chunk| {
				let reader = Arc::clone(&reader);
				async move {
					let big_blob = match reader.read_range(&chunk.range).await {
						Ok(blob) => blob,
						Err(e) => {
							log::error!("failed to read chunk range {:?}: {e}", chunk.range);
							return futures::stream::iter(Vec::new());
						}
					};

					let entries: Vec<(TileCoord, Tile)> = chunk
						.tiles
						.into_iter()
						.map(|(coord, range)| {
							let start = usize::try_from(range.offset - chunk.range.offset)
								.expect("range offset difference should fit in usize");
							let end = start + usize::try_from(range.length).expect("range length should fit in usize");

							let blob = Blob::from(big_blob.range(start..end));
							let tile = Tile::from_blob(blob, tile_compression, tile_format);

							(coord, tile)
						})
						.collect();

					futures::stream::iter(entries)
				}
			})
			.flatten()
			.boxed(),
	)
}
