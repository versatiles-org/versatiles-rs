//! Shared chunking logic for coalescing nearby tile byte ranges into bulk reads.
//!
//! Both the VersaTiles and PMTiles readers use this module to minimize I/O calls
//! when streaming tiles for a bounding box. Nearby byte ranges are grouped into
//! chunks (each up to `VERSATILES_CHUNK_MAX_BYTES`, default 64 MiB, with a small gap
//! tolerance), which are then read as single blobs and sliced into individual tiles.
//!
//! Chunks are read concurrently up to a memory budget (`VERSATILES_CHUNK_READ_MEMORY`,
//! default 256 MiB), so peak read memory is bounded regardless of CPU count — important
//! when a bounding box coalesces into many large chunks.
//!
//! When a chunk read fails, the chunk is automatically split into smaller pieces
//! and retried. This continues recursively down to individual tile reads, so a
//! flaky connection can still make progress instead of failing on large downloads.

use crate::Tile;
use anyhow::Result;
use futures::stream::StreamExt;
use std::sync::Arc;
use versatiles_core::{
	Blob, ByteRange, ConcurrencyLimits, TileCompression, TileCoord, TileFormat, TileStream, io::DataReader,
};

/// Default maximum size of a single coalesced chunk. Each chunk is read as one
/// in-memory blob, so `chunk size × read-ahead` bounds peak read memory. Override
/// with `VERSATILES_CHUNK_MAX_BYTES`.
const DEFAULT_MAX_CHUNK_SIZE: u64 = 64 * 1024 * 1024;
/// Default budget for total in-flight chunk-read bytes. The number of chunks read
/// concurrently is `budget / chunk_size` (≥ 1, capped at the I/O concurrency limit),
/// so peak read memory stays near this value regardless of CPU count. Override with
/// `VERSATILES_CHUNK_READ_MEMORY`.
const DEFAULT_CHUNK_READ_MEMORY: u64 = 256 * 1024 * 1024;
const MAX_CHUNK_GAP: u64 = 256 * 1024;

/// Parse a positive byte-count environment variable, falling back to `default`.
fn env_bytes(name: &str, default: u64) -> u64 {
	std::env::var(name)
		.ok()
		.and_then(|s| s.trim().parse::<u64>().ok())
		.filter(|&n| n > 0)
		.unwrap_or(default)
}

/// Maximum coalesced chunk size (bytes).
fn max_chunk_size() -> u64 {
	env_bytes("VERSATILES_CHUNK_MAX_BYTES", DEFAULT_MAX_CHUNK_SIZE)
}

/// How many chunks to read concurrently, derived from a memory budget so that
/// `concurrency × chunk_size ≈ budget`. Bounds peak read memory independent of the
/// (CPU-derived) I/O concurrency limit, which is far too high for large chunk blobs.
fn chunk_read_concurrency() -> usize {
	let budget = env_bytes("VERSATILES_CHUNK_READ_MEMORY", DEFAULT_CHUNK_READ_MEMORY);
	let by_budget = (budget / max_chunk_size().max(1)).max(1);
	let cap = ConcurrencyLimits::default().io_bound as u64;
	usize::try_from(by_budget.min(cap)).unwrap_or(1).max(1)
}

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
	/// total chunk size stays below [`max_chunk_size`].
	pub fn from_tile_ranges(mut tile_ranges: Vec<(TileCoord, ByteRange)>) -> Chunks {
		Chunks::coalesce(&mut tile_ranges, max_chunk_size(), MAX_CHUNK_GAP)
	}

	/// Convert chunks into a `TileStream` by reading each chunk as a single blob
	/// and slicing out individual tiles.
	pub fn stream(
		self,
		reader: Arc<DataReader>,
		tile_compression: TileCompression,
		tile_format: TileFormat,
	) -> TileStream<'static, Tile> {
		let concurrency = chunk_read_concurrency();
		TileStream::from_stream(
			futures::stream::iter(self.chunks)
				.map(move |chunk| {
					let reader = Arc::clone(&reader);
					async move {
						let entries = chunk
							.read(&reader, tile_compression, tile_format)
							.await
							.unwrap_or_else(|e| panic!("aborting to prevent corrupt output — {e:#}"));
						futures::stream::iter(entries)
					}
				})
				.buffer_unordered(concurrency)
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

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use async_trait::async_trait;
	use std::{
		sync::atomic::{AtomicUsize, Ordering},
		time::Duration,
	};
	use versatiles_core::io::DataReaderTrait;

	/// Shared counters observed from outside the boxed reader.
	#[derive(Debug, Default)]
	struct PeakState {
		in_flight: AtomicUsize,
		max_in_flight: AtomicUsize,
		total_reads: AtomicUsize,
	}

	/// `DataReader` that records peak concurrent in-flight reads via a
	/// shared `Arc<PeakState>`. Each `read_range` increments the counter,
	/// sleeps briefly so overlap is observable, then decrements.
	#[derive(Debug)]
	struct PeakCounter {
		state: Arc<PeakState>,
		delay: Duration,
	}

	#[async_trait]
	impl DataReaderTrait for PeakCounter {
		async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
			let n = self.state.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
			self.state.max_in_flight.fetch_max(n, Ordering::SeqCst);
			self.state.total_reads.fetch_add(1, Ordering::SeqCst);
			tokio::time::sleep(self.delay).await;
			self.state.in_flight.fetch_sub(1, Ordering::SeqCst);
			Ok(Blob::from(vec![0u8; usize::try_from(range.length).unwrap()]))
		}

		async fn read_all(&self) -> Result<Blob> {
			unreachable!("PeakCounter only used for read_range")
		}

		fn name(&self) -> &str {
			"peak-counter"
		}
	}

	#[tokio::test]
	async fn chunks_stream_overlaps_reads() {
		// Build 8 ranges spaced > MAX_CHUNK_GAP apart so each becomes its own chunk.
		let gap = MAX_CHUNK_GAP + 1;
		let tile_ranges: Vec<(TileCoord, ByteRange)> = (0..8u32)
			.map(|i| {
				(
					// Zoom 3 fits 8 x-coords on the row.
					TileCoord::new(3, i, 0).unwrap(),
					ByteRange::new(u64::from(i) * gap, 1),
				)
			})
			.collect();
		let chunks = Chunks::from_tile_ranges(tile_ranges);
		assert_eq!(chunks.chunks.len(), 8, "test setup expects 8 separate chunks");

		let state = Arc::new(PeakState::default());
		let reader: DataReader = Box::new(PeakCounter {
			state: Arc::clone(&state),
			delay: Duration::from_millis(40),
		});

		let _ = chunks
			.stream(Arc::new(reader), TileCompression::Uncompressed, TileFormat::BIN)
			.to_vec()
			.await;

		assert_eq!(state.total_reads.load(Ordering::SeqCst), 8, "one read per chunk");
		let peak = state.max_in_flight.load(Ordering::SeqCst);
		assert!(peak >= 2, "expected concurrent chunk reads, saw peak {peak} in flight");
	}

	#[test]
	fn chunk_concurrency_defaults_to_memory_budget() {
		// With defaults (env unset): 256 MiB budget / 64 MiB chunk = 4, far below io_bound.
		if std::env::var("VERSATILES_CHUNK_MAX_BYTES").is_err() && std::env::var("VERSATILES_CHUNK_READ_MEMORY").is_err()
		{
			assert_eq!(max_chunk_size(), DEFAULT_MAX_CHUNK_SIZE);
			assert_eq!(chunk_read_concurrency(), 4);
			assert!(
				chunk_read_concurrency() < ConcurrencyLimits::default().io_bound,
				"budget must cap concurrency well below the I/O limit"
			);
		}
	}

	#[tokio::test]
	async fn chunks_stream_concurrency_is_bounded() {
		// Many separate chunks, but in-flight reads must stay within the budget cap.
		let gap = MAX_CHUNK_GAP + 1;
		let n = 32u32;
		let tile_ranges: Vec<(TileCoord, ByteRange)> = (0..n)
			.map(|i| (TileCoord::new(5, i, 0).unwrap(), ByteRange::new(u64::from(i) * gap, 1)))
			.collect();
		let chunks = Chunks::from_tile_ranges(tile_ranges);
		assert_eq!(chunks.chunks.len(), n as usize, "expected one chunk per range");

		let state = Arc::new(PeakState::default());
		let reader: DataReader = Box::new(PeakCounter {
			state: Arc::clone(&state),
			delay: Duration::from_millis(20),
		});

		let _ = chunks
			.stream(Arc::new(reader), TileCompression::Uncompressed, TileFormat::BIN)
			.to_vec()
			.await;

		let peak = state.max_in_flight.load(Ordering::SeqCst);
		let limit = chunk_read_concurrency();
		assert!(
			peak <= limit,
			"peak {peak} in flight exceeded the budget concurrency {limit}"
		);
	}
}
