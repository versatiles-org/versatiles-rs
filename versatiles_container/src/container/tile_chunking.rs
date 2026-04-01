//! Shared chunking logic for coalescing nearby tile byte ranges into bulk reads.
//!
//! Both the VersaTiles and PMTiles readers use this module to minimize I/O calls
//! when streaming tiles for a bounding box. Nearby byte ranges are grouped into
//! chunks (up to ~256 MiB each, with a small gap tolerance), which are then read
//! as single large blobs and sliced into individual tiles.
//!
//! When a chunk read fails, the chunk is automatically split into smaller pieces
//! and retried. This continues recursively down to individual tile reads, so a
//! flaky connection can still make progress instead of failing on large downloads.

use crate::Tile;
use anyhow::Result;
use futures::stream::StreamExt;
use std::sync::Arc;
use versatiles_core::{Blob, ByteRange, TileCompression, TileCoord, TileFormat, TileStream, io::DataReader};

const MAX_CHUNK_SIZE: u64 = 256 * 1024 * 1024;
const MAX_CHUNK_GAP: u64 = 256 * 1024;

/// A group of tile byte ranges that can be served from a single large read.
/// `range` tracks the combined byte span in the container.
#[derive(Debug)]
pub struct Chunk {
	tiles: Vec<(TileCoord, ByteRange)>,
	range: ByteRange,
}

impl Chunk {
	fn new(start: u64) -> Self {
		Self {
			tiles: Vec::new(),
			range: ByteRange::new(start, 0),
		}
	}

	fn push(&mut self, entry: (TileCoord, ByteRange)) {
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

	/// Read a chunk from the reader, slicing it into individual tiles.
	///
	/// On failure, the chunk is split into progressively smaller sub-chunks and
	/// retried. This means a flaky connection that can't sustain a 200 MB download
	/// can still succeed by reading smaller pieces.
	async fn read(
		&self,
		reader: &DataReader,
		tile_compression: TileCompression,
		tile_format: TileFormat,
	) -> Result<Vec<(TileCoord, Tile)>> {
		let big_blob = reader.read_range(&self.range).await?;
		Ok(self.slice_tiles(&big_blob, tile_compression, tile_format))
	}

	/// Slice a big blob into individual tiles using the chunk's tile ranges.
	fn slice_tiles(
		&self,
		big_blob: &Blob,
		tile_compression: TileCompression,
		tile_format: TileFormat,
	) -> Vec<(TileCoord, Tile)> {
		let chunk_start = self.range.offset;
		self
			.tiles
			.iter()
			.map(|(coord, range)| {
				let start =
					usize::try_from(range.offset - chunk_start).expect("range offset difference should fit in usize");
				let end = start + usize::try_from(range.length).expect("range length should fit in usize");

				let blob = Blob::from(big_blob.range(start..end));
				let tile = Tile::from_blob(blob, tile_compression, tile_format);

				(*coord, tile)
			})
			.collect()
	}
}

pub struct Chunks {
	chunks: Vec<Chunk>,
}

impl Chunks {
	fn new(chunks: Vec<Chunk>) -> Self {
		Self { chunks }
	}

	pub fn new_empty() -> Self {
		Self { chunks: Vec::new() }
	}

	/// Sort tile ranges by byte offset and coalesce into chunks.
	///
	/// Nearby ranges (within `max_gap`) are grouped together as long as the
	/// total chunk size stays below `max_size`.
	fn coalesce(tile_ranges: &mut Vec<(TileCoord, ByteRange)>, max_size: u64, max_gap: u64) -> Chunks {
		if tile_ranges.is_empty() {
			return Chunks::new(Vec::new());
		}

		tile_ranges.sort_by_key(|e| e.1.offset);

		let mut chunks: Vec<Chunk> = Vec::new();
		let mut chunk = Chunk::new(tile_ranges[0].1.offset);

		for entry in tile_ranges.drain(..) {
			let chunk_start = chunk.range.offset;
			let chunk_end = chunk.range.offset + chunk.range.length;

			let tile_start = entry.1.offset;
			let tile_end = entry.1.offset + entry.1.length;

			if (chunk_start + max_size > tile_end) && (chunk_end + max_gap > tile_start) {
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

		Chunks::new(chunks)
	}

	/// Sort tile ranges by byte offset and coalesce into chunks.
	///
	/// Nearby ranges (within `MAX_CHUNK_GAP`) are grouped together as long as the
	/// total chunk size stays below `MAX_CHUNK_SIZE`.
	pub fn from_tile_ranges(mut tile_ranges: Vec<(TileCoord, ByteRange)>) -> Chunks {
		Chunks::coalesce(&mut tile_ranges, MAX_CHUNK_SIZE, MAX_CHUNK_GAP)
	}

	/// Convert chunks into a `TileStream` by reading each chunk as a single blob
	/// and slicing out individual tiles.
	pub fn stream(
		self,
		reader: Arc<DataReader>,
		tile_compression: TileCompression,
		tile_format: TileFormat,
	) -> TileStream<'static, Tile> {
		TileStream::from_stream(
			futures::stream::iter(self.chunks)
				.then(move |chunk| {
					let reader = Arc::clone(&reader);
					async move {
						let entries = chunk
							.read(&reader, tile_compression, tile_format)
							.await
							.unwrap_or_else(|e| panic!("aborting to prevent corrupt output — {e:#}"));
						futures::stream::iter(entries)
					}
				})
				.flatten()
				.boxed(),
		)
	}
}

impl IntoIterator for Chunks {
	type Item = Chunk;
	type IntoIter = std::vec::IntoIter<Self::Item>;

	fn into_iter(self) -> Self::IntoIter {
		self.chunks.into_iter()
	}
}

impl FromIterator<Chunk> for Chunks {
	fn from_iter<T: IntoIterator<Item = Chunk>>(iter: T) -> Self {
		Chunks::new(iter.into_iter().collect())
	}
}
