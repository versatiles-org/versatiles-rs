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
//! A failing read is retried one layer down, not here: `NetworkReader` splits an
//! over-large range in half and reads each part, learning the limit for later
//! ranges, so a flaky connection still makes progress on large downloads. A file
//! reader has nothing to retry, and either way a read that ends up failing is
//! reported to the runtime by [`Chunks::stream`] and its tiles dropped.

use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use futures::stream::StreamExt;
use versatiles_core::{
	Blob, ByteRange, ConcurrencyLimits, TileCompression, TileCoord, TileFormat, TileStream, io::DataReader,
};

use crate::{Tile, TilesRuntime};

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
		// Saturating throughout `coalesce`: grouping tiles into chunks is a
		// heuristic over offsets read from a tile index, and an absurd offset
		// should leave a tile ungrouped rather than panic in a debug build or
		// wrap in a release one. The read itself is bounds-checked either way.
		self.range.length = self.range.length.max(
			entry
				.1
				.offset
				.saturating_add(entry.1.length)
				.saturating_sub(self.range.offset),
		);
		self.tiles.push(entry);
	}

	/// Read a chunk from the reader, slicing it into individual tiles.
	///
	/// Retrying is the reader's business, not this function's: a `NetworkReader`
	/// splits a range it could not fetch and reads the halves, so a connection
	/// that cannot sustain a 200 MB download still succeeds in pieces. An `Err`
	/// here therefore means the read failed with whatever retries that reader
	/// had — see [`Chunks::stream`] for what happens to the chunk's tiles then.
	async fn read(
		&self,
		reader: &DataReader,
		tile_compression: TileCompression,
		tile_format: TileFormat,
	) -> Result<Vec<(TileCoord, Tile)>> {
		log::trace!(
			"chunk: reading {} bytes for {} tiles",
			self.range.length,
			self.tiles.len()
		);
		let big_blob = reader.read_range(&self.range).await?;
		log::trace!(
			"chunk: slicing {} tiles from {} bytes (range offset {})",
			self.tiles.len(),
			big_blob.len(),
			self.range.offset
		);
		let tiles = self.slice_tiles(&big_blob, tile_compression, tile_format)?;
		log::trace!("chunk: done slicing {} tiles", tiles.len());
		Ok(tiles)
	}

	/// Slice a big blob into individual tiles using the chunk's tile ranges.
	///
	/// Fallible because every number involved comes out of the container's
	/// index. `offset - chunk_start` underflows if a tile claims to start before
	/// the chunk holding it, `start + length` can leave the blob entirely, and
	/// `Blob::range` is an unchecked slice — so on a crafted index this used to
	/// panic in a debug build and, with the subtraction wrapping in release,
	/// index far outside the blob. Each tile is checked against the bytes
	/// actually read instead.
	fn slice_tiles(
		&self,
		big_blob: &Blob,
		tile_compression: TileCompression,
		tile_format: TileFormat,
	) -> Result<Vec<(TileCoord, Tile)>> {
		let chunk_start = self.range.offset;
		self
			.tiles
			.iter()
			.map(|(coord, range)| {
				let offset_in_chunk = range.offset.checked_sub(chunk_start).with_context(|| {
					format!(
						"tile {coord:?} starts at {}, before the chunk at {chunk_start} that should contain it",
						range.offset
					)
				})?;
				let start = usize::try_from(offset_in_chunk).context("tile offset too large for this platform")?;
				let length = usize::try_from(range.length).context("tile length too large for this platform")?;
				let end = start
					.checked_add(length)
					.with_context(|| format!("tile {coord:?} range {start}+{length} overflows"))?;

				ensure!(
					end as u64 <= big_blob.len(),
					"tile {coord:?} ends at {end}, past the {} bytes read for its chunk",
					big_blob.len()
				);

				let blob = Blob::from(big_blob.range(start..end));
				let tile = Tile::from_blob(blob, tile_compression, tile_format);

				Ok((*coord, tile))
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
			let chunk_end = chunk.range.offset.saturating_add(chunk.range.length);

			let tile_start = entry.1.offset;
			let tile_end = entry.1.offset.saturating_add(entry.1.length);

			if (chunk_start.saturating_add(max_size) > tile_end) && (chunk_end.saturating_add(max_gap) > tile_start) {
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
	///
	/// A chunk that cannot be read is reported to `runtime` and its tiles are
	/// dropped. Whether that ends the run is the caller's policy, not this
	/// function's: `convert` sets
	/// `abort_on_error` and fails once the stream has drained, so no truncated
	/// output is written, while a server records the error and keeps serving the
	/// tiles it can read.
	///
	/// This used to `panic!` instead, to avoid writing a corrupt output file. The
	/// intent was right and the mechanism was not — the panic ran on a runtime
	/// worker inside `buffer_unordered`, so any tile index entry pointing past
	/// EOF took down a thread of whatever process was hosting the library.
	pub fn stream(
		self,
		reader: Arc<DataReader>,
		tile_compression: TileCompression,
		tile_format: TileFormat,
		runtime: &TilesRuntime,
	) -> TileStream<'static, Tile> {
		let concurrency = chunk_read_concurrency();
		log::trace!(
			"chunk stream: {} chunks, read concurrency {concurrency}",
			self.chunks.len()
		);
		let runtime = runtime.clone();
		TileStream::from_stream(
			futures::stream::iter(self.chunks)
				.map(move |chunk| {
					let reader = Arc::clone(&reader);
					let runtime = runtime.clone();
					async move {
						let range = chunk.range;
						let tile_count = chunk.tiles.len();
						let entries = match chunk.read(&reader, tile_compression, tile_format).await {
							Ok(entries) => entries,
							Err(e) => {
								runtime.record_error(
									"chunk read",
									&e.context(format!("reading {tile_count} tiles from range {range:?}")),
								);
								Vec::new()
							}
						};
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
	use std::{
		sync::atomic::{AtomicUsize, Ordering},
		time::Duration,
	};

	use async_trait::async_trait;
	use versatiles_core::io::DataReaderTrait;

	use super::*;

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
			.stream(
				Arc::new(reader),
				TileCompression::Uncompressed,
				TileFormat::BIN,
				&TilesRuntime::new_silent(),
			)
			.to_vec()
			.await;

		assert_eq!(state.total_reads.load(Ordering::SeqCst), 8, "one read per chunk");
		let peak = state.max_in_flight.load(Ordering::SeqCst);
		assert!(peak >= 2, "expected concurrent chunk reads, saw peak {peak} in flight");
	}

	/// `DataReader` that fails every read, standing in for the crafted index
	/// entry that points past EOF.
	#[derive(Debug)]
	struct AlwaysFails;

	#[async_trait]
	impl DataReaderTrait for AlwaysFails {
		async fn read_range(&self, range: &ByteRange) -> Result<Blob> {
			anyhow::bail!("range {range:?} is past the end of the file")
		}

		async fn read_all(&self) -> Result<Blob> {
			unreachable!("AlwaysFails only used for read_range")
		}

		fn name(&self) -> &str {
			"always-fails"
		}
	}

	fn one_failing_chunk() -> Chunks {
		Chunks::from_tile_ranges(vec![(TileCoord::new(3, 0, 0).unwrap(), ByteRange::new(0, 1))])
	}

	/// This used to be `panic!("aborting to prevent corrupt output")` on a
	/// runtime worker inside `buffer_unordered`. The intent — never write a
	/// truncated file — is now carried by the runtime, which is what lets the
	/// *caller* decide, so the two halves are asserted separately below.
	#[tokio::test]
	async fn a_failing_chunk_is_recorded_rather_than_panicking() {
		let runtime = TilesRuntime::new_silent();
		let reader: DataReader = Box::new(AlwaysFails);

		let tiles = one_failing_chunk()
			.stream(
				Arc::new(reader),
				TileCompression::Uncompressed,
				TileFormat::BIN,
				&runtime,
			)
			.to_vec()
			.await;

		assert!(tiles.is_empty(), "no tile can come out of a chunk that did not read");
		assert_eq!(runtime.error_count(), 1, "the failure must not be swallowed");
	}

	/// The half that replaces the panic: a conversion still refuses to write
	/// truncated output, it just does so after the stream drains instead of from
	/// inside a future.
	#[tokio::test]
	async fn a_converting_caller_still_refuses_the_truncated_result() {
		let converting = TilesRuntime::new_silent();
		converting.set_abort_on_error(true);
		let reader: DataReader = Box::new(AlwaysFails);
		let _ = one_failing_chunk()
			.stream(
				Arc::new(reader),
				TileCompression::Uncompressed,
				TileFormat::BIN,
				&converting,
			)
			.to_vec()
			.await;
		assert!(converting.had_errors(), "convert must have something to fail on");

		// The same failure, the other policy: a server drops the tile and keeps
		// answering the requests it can.
		let serving = TilesRuntime::new_silent();
		let reader: DataReader = Box::new(AlwaysFails);
		let _ = one_failing_chunk()
			.stream(
				Arc::new(reader),
				TileCompression::Uncompressed,
				TileFormat::BIN,
				&serving,
			)
			.to_vec()
			.await;
		assert!(!serving.had_errors(), "a server keeps going");
		assert_eq!(serving.error_count(), 1, "but the error is still reported");
	}

	/// The message has to name what was being read, or a truncated container is
	/// a mystery rather than a diagnosis.
	#[tokio::test]
	async fn the_error_says_what_could_not_be_read() {
		let runtime = TilesRuntime::new_silent();
		let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
		let sink = Arc::clone(&seen);
		runtime.events().subscribe(move |event| {
			if let crate::Event::Error { message } = event {
				sink.lock().unwrap().push(message.clone());
			}
		});

		let reader: DataReader = Box::new(AlwaysFails);
		let _ = one_failing_chunk()
			.stream(
				Arc::new(reader),
				TileCompression::Uncompressed,
				TileFormat::BIN,
				&runtime,
			)
			.to_vec()
			.await;

		let messages = seen.lock().unwrap();
		let message = messages.first().expect("an error event was emitted");
		assert!(message.contains("chunk read"), "names the producer: {message}");
		assert!(message.contains("1 tiles"), "names the scope: {message}");
		assert!(message.contains("past the end of the file"), "carries why: {message}");
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
			.stream(
				Arc::new(reader),
				TileCompression::Uncompressed,
				TileFormat::BIN,
				&TilesRuntime::new_silent(),
			)
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

#[cfg(test)]
mod slice_bounds_tests {
	use super::*;

	fn chunk_with(range: ByteRange, tiles: Vec<(TileCoord, ByteRange)>) -> Chunk {
		Chunk { tiles, range }
	}

	/// Every number here comes out of the container's index. A tile claiming to
	/// start before the chunk that holds it underflowed the offset subtraction —
	/// a panic in a debug build, and in release a wrapped value that indexed far
	/// outside the blob through `Blob::range`, which does not bounds-check.
	#[test]
	fn a_tile_starting_before_its_chunk_is_an_error() {
		let coord = TileCoord::new(0, 0, 0).unwrap();
		let chunk = chunk_with(ByteRange::new(100, 50), vec![(coord, ByteRange::new(40, 10))]);

		let error = chunk
			.slice_tiles(
				&Blob::from(vec![0u8; 50]),
				TileCompression::Uncompressed,
				TileFormat::PNG,
			)
			.unwrap_err();

		assert!(format!("{error:#}").contains("before the chunk"), "{error:#}");
	}

	/// A tile whose end runs past what was actually read would slice out of
	/// bounds.
	#[test]
	fn a_tile_ending_past_the_chunk_is_an_error() {
		let coord = TileCoord::new(0, 0, 0).unwrap();
		let chunk = chunk_with(ByteRange::new(100, 50), vec![(coord, ByteRange::new(140, 999))]);

		let error = chunk
			.slice_tiles(
				&Blob::from(vec![0u8; 50]),
				TileCompression::Uncompressed,
				TileFormat::PNG,
			)
			.unwrap_err();

		assert!(format!("{error:#}").contains("past the 50 bytes"), "{error:#}");
	}

	/// The ordinary case still slices, including a tile that ends exactly on the
	/// last byte read.
	#[test]
	fn tiles_inside_the_chunk_are_sliced() {
		let a = TileCoord::new(0, 0, 0).unwrap();
		let b = TileCoord::new(1, 1, 1).unwrap();
		let chunk = chunk_with(
			ByteRange::new(100, 50),
			vec![(a, ByteRange::new(100, 10)), (b, ByteRange::new(140, 10))],
		);

		let tiles = chunk
			.slice_tiles(
				&Blob::from(vec![7u8; 50]),
				TileCompression::Uncompressed,
				TileFormat::PNG,
			)
			.unwrap();

		assert_eq!(tiles.len(), 2);
		assert_eq!(tiles[0].0, a);
		assert_eq!(tiles[1].0, b);
	}
}
