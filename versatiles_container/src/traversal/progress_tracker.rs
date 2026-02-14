//! Progress tracking utilities for tile traversal operations.
//!
//! This module provides [`ProgressTracker`], a helper struct for consistent
//! progress tracking using the midpoint of read/write operations.

use crate::ProgressHandle;
use std::sync::atomic::{AtomicU64, Ordering};

/// Helper for consistent progress tracking using midpoint of read/write operations.
///
/// The tracker maintains separate counters for read and write operations, plus an
/// offset counter for intermediate progress. The reported position is always the
/// midpoint: `(read + write + offset) / 2`.
///
/// This approach provides smooth progress indication when read and write operations
/// happen at different rates or in separate phases (e.g., Push/Pop caching).
pub(crate) struct ProgressTracker {
	progress: ProgressHandle,
	ti_read: AtomicU64,
	ti_write: AtomicU64,
	ti_offset: AtomicU64,
}

impl ProgressTracker {
	/// Create a new progress tracker wrapping the given progress handle.
	pub fn new(progress: ProgressHandle) -> Self {
		Self {
			progress,
			ti_read: AtomicU64::new(0),
			ti_write: AtomicU64::new(0),
			ti_offset: AtomicU64::new(0),
		}
	}

	/// Increment the offset counter by the given count.
	///
	/// Used for intermediate progress within a read or write phase.
	pub fn inc(&self, count: u64) {
		self.ti_offset.fetch_add(count, Ordering::Relaxed);
		self.update_position();
	}

	/// Increment the read counter and reset the offset.
	///
	/// Called when a read operation completes.
	pub fn inc_read(&self, value: u64) {
		self.ti_read.fetch_add(value, Ordering::Relaxed);
		self.ti_offset.store(0, Ordering::Relaxed);
		self.update_position();
	}

	/// Increment the write counter and reset the offset.
	///
	/// Called when a write operation completes.
	pub fn inc_write(&self, value: u64) {
		self.ti_write.fetch_add(value, Ordering::Relaxed);
		self.ti_offset.store(0, Ordering::Relaxed);
		self.update_position();
	}

	/// Increment both read and write counters and reset the offset.
	///
	/// Called when a streaming operation completes (simultaneous read+write).
	pub fn inc_read_write(&self, read: u64, write: u64) {
		self.ti_read.fetch_add(read, Ordering::Relaxed);
		self.ti_write.fetch_add(write, Ordering::Relaxed);
		self.ti_offset.store(0, Ordering::Relaxed);
		self.update_position();
	}

	/// Update the progress handle with the current midpoint position.
	fn update_position(&self) {
		let read = self.ti_read.load(Ordering::Relaxed);
		let write = self.ti_write.load(Ordering::Relaxed);
		let offset = self.ti_offset.load(Ordering::Relaxed);
		self.progress.set_position((read + write + offset) / 2);
	}

	/// Mark the progress as finished.
	pub fn finish(&self) {
		self.progress.finish();
	}
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
mod tests {
	use crate::{SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime};
	use anyhow::Result;
	use async_trait::async_trait;
	use std::sync::Arc;
	use versatiles_core::{Blob, TileBBox, TileBBoxPyramid, TileCompression, TileFormat, TileJSON, TileStream};

	use super::super::{TileSourceTraverseExt, Traversal};

	// ============================================================================
	// Test helpers
	// ============================================================================

	/// Test reader that produces tiles with predictable content.
	#[derive(Debug)]
	struct TestReader {
		metadata: TileSourceMetadata,
		tilejson: TileJSON,
		/// Optional delay in microseconds before returning each tile.
		tile_delay_micros: u64,
	}

	impl TestReader {
		fn new(traversal: Traversal, max_level: u8) -> Self {
			TestReader {
				metadata: TileSourceMetadata {
					bbox_pyramid: TileBBoxPyramid::new_full_up_to(max_level),
					tile_compression: TileCompression::Uncompressed,
					tile_format: TileFormat::PNG,
					traversal,
				},
				tilejson: TileJSON::default(),
				tile_delay_micros: 0,
			}
		}
	}

	async fn sleep_micros(micros: u64) {
		tokio::time::sleep(std::time::Duration::from_micros(micros)).await;
	}

	#[async_trait]
	impl TileSource for TestReader {
		fn source_type(&self) -> Arc<SourceType> {
			SourceType::new_container("test", "test://")
		}

		fn metadata(&self) -> &TileSourceMetadata {
			&self.metadata
		}

		fn tilejson(&self) -> &TileJSON {
			&self.tilejson
		}

		async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
			let compression = self.metadata.tile_compression;
			let format = self.metadata.tile_format;
			let delay_micros = self.tile_delay_micros;

			if delay_micros > 0 {
				// Use async stream with delay
				Ok(TileStream::from_bbox_async_parallel(bbox, move |coord| async move {
					sleep_micros(delay_micros).await;
					let data = format!("tile:{},{},{}", coord.level, coord.x, coord.y);
					Some((coord, Tile::from_blob(Blob::from(data), compression, format)))
				}))
			} else {
				// Fast path without delay
				Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |coord| {
					let data = format!("tile:{},{},{}", coord.level, coord.x, coord.y);
					Some(Tile::from_blob(Blob::from(data), compression, format))
				}))
			}
		}
	}

	/// A captured progress event with timestamp.
	#[derive(Debug, Clone)]
	struct ProgressEvent {
		total: u64,
	}

	/// Helper to capture progress events with timestamps.
	fn setup_progress_capture(runtime: &TilesRuntime) -> Arc<std::sync::Mutex<Vec<ProgressEvent>>> {
		let events = Arc::new(std::sync::Mutex::new(Vec::new()));
		let events_clone = events.clone();

		runtime.events().subscribe(move |event| {
			if let crate::Event::Progress { data } = event {
				events_clone.lock().unwrap().push(ProgressEvent { total: data.total });
			}
		});

		events
	}

	// ============================================================================
	// Progress tracking tests
	// ============================================================================

	#[tokio::test]
	async fn test_progress_position_matches_expected_formula() -> Result<()> {
		// Verify that the final total matches the expected formula:
		// total = midpoint(tn_read, tn_write) = (tn_read + tn_write) / 2
		// For streaming ANY->ANY, tn_read == tn_write == tile_count
		// So total = (tile_count + tile_count) / 2 = tile_count

		let reader = TestReader::new(Traversal::ANY, 2);
		let runtime = TilesRuntime::builder()
			.with_memory_cache()
			.silent_progress(true)
			.build();

		let positions = setup_progress_capture(&runtime);

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				|_bbox, stream| {
					Box::pin(async move {
						stream.for_each_async(|_, _| sleep_micros(100000)).await;
						Ok(())
					})
				},
				runtime,
				None,
			)
			.await?;

		let captured = positions.lock().unwrap();
		let last = captured.last().unwrap();

		// Levels 0-2: 1 + 4 + 16 = 21 tiles
		// For streaming mode: total = midpoint(21, 21) = 21
		assert_eq!(last.total, 21, "Total should be 21 for levels 0-2 in streaming mode");

		Ok(())
	}
}
