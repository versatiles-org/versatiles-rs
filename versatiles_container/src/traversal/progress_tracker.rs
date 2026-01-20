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
		self.update_position();
		self.progress.finish();
	}
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
mod tests {
	use crate::{SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime, TraversalOrder};
	use anyhow::Result;
	use async_trait::async_trait;
	use rstest::rstest;
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
					bbox_pyramid: TileBBoxPyramid::new_full(max_level),
					tile_compression: TileCompression::Uncompressed,
					tile_format: TileFormat::PNG,
					traversal,
				},
				tilejson: TileJSON::default(),
				tile_delay_micros: 0,
			}
		}

		fn with_level_range(traversal: Traversal, min_level: u8, max_level: u8) -> Self {
			use versatiles_core::GeoBBox;
			// Use full world bbox to get all tiles at each zoom level
			let bbox = GeoBBox::new(-180.0, -85.05, 180.0, 85.05).unwrap();
			TestReader {
				metadata: TileSourceMetadata {
					bbox_pyramid: TileBBoxPyramid::from_geo_bbox(min_level, max_level, &bbox),
					tile_compression: TileCompression::Uncompressed,
					tile_format: TileFormat::PNG,
					traversal,
				},
				tilejson: TileJSON::default(),
				tile_delay_micros: 0,
			}
		}

		/// Set the delay in microseconds before returning each tile.
		fn with_tile_delay_micros(mut self, delay_micros: u64) -> Self {
			self.tile_delay_micros = delay_micros;
			self
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
		position: u64,
		total: u64,
		finished: bool,
		timestamp: std::time::Instant,
	}

	/// Helper to capture progress events with timestamps.
	fn setup_progress_capture(runtime: &TilesRuntime) -> Arc<std::sync::Mutex<Vec<ProgressEvent>>> {
		let events = Arc::new(std::sync::Mutex::new(Vec::new()));
		let events_clone = events.clone();

		runtime.events().subscribe(move |event| {
			if let crate::Event::Progress { data } = event {
				events_clone.lock().unwrap().push(ProgressEvent {
					position: data.position,
					total: data.total,
					finished: data.finished,
					timestamp: std::time::Instant::now(),
				});
			}
		});

		events
	}

	/// Verify that progress positions are monotonically non-decreasing and grow linearly over time.
	///
	/// Checks:
	/// 1. Positions never decrease (monotonicity)
	/// 2. No single step exceeds 10% of total progress
	/// 3. Maximum deviation from linear progress is within the allowed threshold
	///
	/// # Arguments
	/// * `events` - The captured progress events
	/// * `max_deviation` - Maximum allowed relative deviation from linear (0.0 = perfect, 1.0 = can be anywhere)
	fn verify_monotonic_progress(events: &[ProgressEvent], max_deviation: f64) {
		assert!(!events.is_empty(), "Should have at least one progress event");

		let total = events.last().unwrap().total as f64;

		// Verify monotonicity and step sizes
		let mut prev_position = 0u64;
		for (i, event) in events.iter().enumerate() {
			assert!(
				event.position >= prev_position,
				"Progress position decreased at index {i}: {prev_position} -> {}",
				event.position
			);
			assert!(
				event.position <= event.total,
				"Progress position {} exceeds total {} at index {i}",
				event.position,
				event.total
			);

			// Check that step size is not bigger than 10% of total
			// Skip this check if total is too small for meaningful granularity (< 10 items)
			if total >= 10.0 {
				let step = event.position - prev_position;
				if step <= 1 {
					continue;
				}
				let relative_step = step as f64 / total;
				assert!(
					relative_step <= 0.1,
					"Progress step too large at index {i}: step={step} ({:.1}% of total), prev={prev_position}, current={}",
					relative_step * 100.0,
					event.position
				);
			}

			prev_position = event.position;
		}

		// Need at least 2 events to check linearity
		if events.len() < 2 {
			return;
		}

		let first = events.first().unwrap();
		let last = events.last().unwrap();

		let total_time = last.timestamp.duration_since(first.timestamp).as_secs_f64();
		let total_progress = last.total as f64;

		// Skip linearity check if total time is too short (< 1ms) or no progress
		if total_time < 0.001 || total_progress == 0.0 {
			return;
		}

		// Find maximum relative deviation from linear progress
		let mut max_relative_deviation = 0.0f64;
		for event in events {
			let elapsed = event.timestamp.duration_since(first.timestamp).as_secs_f64();
			let time_fraction = elapsed / total_time;
			let expected_position = time_fraction * total_progress;
			let actual_position = event.position as f64;
			let relative_deviation = (actual_position - expected_position).abs() / total_progress;
			max_relative_deviation = max_relative_deviation.max(relative_deviation);
		}

		assert!(
			max_relative_deviation <= max_deviation,
			"Progress deviates too much from linear: max_deviation={max_relative_deviation:.3} (allowed: {max_deviation:.3})"
		);
	}

	/// Verify that progress finished correctly
	fn verify_progress_finished(events: &[ProgressEvent]) {
		assert!(!events.is_empty(), "Should have received at least one progress event");

		let last = events.last().unwrap();
		assert!(last.finished, "Final progress event should be marked as finished");
		assert_eq!(
			last.position, last.total,
			"Final position should equal total: {} != {}",
			last.position, last.total
		);
	}

	// ============================================================================
	// Progress tracking tests
	// ============================================================================

	#[tokio::test]
	async fn test_progress_tracking_streaming_mode() -> Result<()> {
		// Test progress tracking when tiles are streamed directly (no caching)
		let reader = TestReader::new(Traversal::ANY, 2).with_tile_delay_micros(10000);
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
						stream.for_each_async(|_| sleep_micros(10000)).await;
						Ok(())
					})
				},
				runtime,
				Some("progress test streaming"),
			)
			.await?;

		let captured = positions.lock().unwrap();
		verify_monotonic_progress(&captured, 0.1);
		verify_progress_finished(&captured);

		// Verify we got multiple progress updates (not just start and finish)
		assert!(
			captured.len() >= 2,
			"Should have at least initial and final progress events, got {}",
			captured.len()
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_progress_tracking_with_caching() -> Result<()> {
		// Test progress tracking with Push/Pop caching path
		let source_traversal = Traversal::new(TraversalOrder::DepthFirst, 1, 8)?;
		let write_traversal = Traversal::new(TraversalOrder::AnyOrder, 16, 16)?;

		// Use zoom levels 4-5 for sufficient tile count
		let reader = TestReader::with_level_range(source_traversal, 4, 5).with_tile_delay_micros(1);
		let runtime = TilesRuntime::builder()
			.with_memory_cache()
			.silent_progress(true)
			.build();

		let positions = setup_progress_capture(&runtime);

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					Box::pin(async move {
						stream.for_each_async_parallel(|_| sleep_micros(1)).await;
						Ok(())
					})
				},
				runtime,
				Some("progress test caching"),
			)
			.await?;

		let captured = positions.lock().unwrap();
		verify_monotonic_progress(&captured, 0.1);
		verify_progress_finished(&captured);

		Ok(())
	}

	#[tokio::test]
	async fn test_progress_tracking_with_disk_cache() -> Result<()> {
		// Test progress tracking with disk-backed caching
		let source_traversal = Traversal::new(TraversalOrder::DepthFirst, 1, 8)?;
		let write_traversal = Traversal::new(TraversalOrder::AnyOrder, 16, 16)?;

		let reader = TestReader::with_level_range(source_traversal, 4, 5).with_tile_delay_micros(1);
		let temp_dir = tempfile::TempDir::new()?;
		let runtime = TilesRuntime::builder()
			.with_disk_cache(temp_dir.path())
			.silent_progress(true)
			.build();

		let positions = setup_progress_capture(&runtime);

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					Box::pin(async move {
						stream.for_each_async_parallel(|_| sleep_micros(1)).await;
						Ok(())
					})
				},
				runtime,
				Some("progress test disk cache"),
			)
			.await?;

		let captured = positions.lock().unwrap();
		verify_monotonic_progress(&captured, 0.1);
		verify_progress_finished(&captured);

		Ok(())
	}

	/// Test progress tracking with various traversal order combinations using rstest
	#[rstest]
	#[case::any_to_any(TraversalOrder::AnyOrder, 1, 256, TraversalOrder::AnyOrder, 1, 256, 4, 5)]
	#[case::any_to_depthfirst(TraversalOrder::AnyOrder, 1, 256, TraversalOrder::DepthFirst, 1, 256, 4, 5)]
	#[case::depthfirst_to_any_cached(TraversalOrder::DepthFirst, 1, 8, TraversalOrder::AnyOrder, 16, 16, 4, 5)]
	#[case::pmtiles_to_any_cached(TraversalOrder::PMTiles, 1, 8, TraversalOrder::AnyOrder, 16, 16, 4, 5)]
	#[tokio::test]
	async fn test_progress_tracking_various_traversals(
		#[case] source_order: TraversalOrder,
		#[case] source_min_size: u32,
		#[case] source_max_size: u32,
		#[case] write_order: TraversalOrder,
		#[case] write_min_size: u32,
		#[case] write_max_size: u32,
		#[case] min_level: u8,
		#[case] max_level: u8,
	) -> Result<()> {
		let source_traversal = Traversal::new(source_order, source_min_size, source_max_size)?;
		let write_traversal = Traversal::new(write_order, write_min_size, write_max_size)?;

		let reader = TestReader::with_level_range(source_traversal, min_level, max_level).with_tile_delay_micros(1000);
		let runtime = TilesRuntime::builder()
			.with_memory_cache()
			.silent_progress(true)
			.build();

		let positions = setup_progress_capture(&runtime);

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					Box::pin(async move {
						stream.for_each_async_parallel(|_| sleep_micros(1000)).await;
						Ok(())
					})
				},
				runtime,
				None,
			)
			.await?;

		let captured = positions.lock().unwrap();
		verify_monotonic_progress(&captured, 0.1);
		verify_progress_finished(&captured);

		Ok(())
	}

	#[tokio::test]
	async fn test_progress_tracking_single_tile() -> Result<()> {
		// Test progress tracking with minimal tile count (level 0 only = 1 tile)
		let reader = TestReader::new(Traversal::ANY, 0).with_tile_delay_micros(10000);
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
						stream.for_each_async(|_| sleep_micros(10000)).await;
						Ok(())
					})
				},
				runtime,
				Some("single tile progress"),
			)
			.await?;

		let captured = positions.lock().unwrap();
		verify_monotonic_progress(&captured, 0.6);
		verify_progress_finished(&captured);

		// For single tile, total should be 1 (midpoint of 1 read + 1 write = 1)
		let last = captured.last().unwrap();
		assert_eq!(last.total, 1, "Total for single tile should be 1");

		Ok(())
	}

	#[tokio::test]
	async fn test_progress_tracking_larger_tile_count() -> Result<()> {
		// Test with larger tile count to ensure progress increments multiple times
		// Level 6 has 4096 tiles, total 5461 tiles across levels 0-6
		let reader = TestReader::new(Traversal::ANY, 6).with_tile_delay_micros(1);
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
						stream.for_each_async_parallel(|_| sleep_micros(1)).await;
						Ok(())
					})
				},
				runtime,
				Some("large tile count progress"),
			)
			.await?;

		let captured = positions.lock().unwrap();
		verify_monotonic_progress(&captured, 0.1);
		verify_progress_finished(&captured);

		// Verify progress actually increased over time
		if captured.len() >= 2 {
			let first_pos = captured.first().unwrap().position;
			let last_pos = captured.last().unwrap().position;
			assert!(
				last_pos > first_pos,
				"Progress should have increased from first to last: {first_pos} -> {last_pos}"
			);
		}

		Ok(())
	}

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
						stream.for_each_async(|_| sleep_micros(100000)).await;
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

	#[tokio::test]
	async fn test_progress_tracking_push_pop_formula() -> Result<()> {
		// Verify progress formula for Push/Pop caching path
		// Push reads tiles, Pop writes them - they happen separately
		let source_traversal = Traversal::new(TraversalOrder::DepthFirst, 1, 8)?;
		let write_traversal = Traversal::new(TraversalOrder::AnyOrder, 16, 16)?;

		// Use zoom levels 4-4 (single level) for predictable counts
		// Level 4 has 16x16 = 256 tiles
		let reader = TestReader::with_level_range(source_traversal, 4, 4).with_tile_delay_micros(500);
		let runtime = TilesRuntime::builder()
			.with_memory_cache()
			.silent_progress(true)
			.build();

		let positions = setup_progress_capture(&runtime);

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					Box::pin(async move {
						stream.for_each_async(|_| sleep_micros(500)).await;
						Ok(())
					})
				},
				runtime,
				Some("push/pop formula test"),
			)
			.await?;

		let captured = positions.lock().unwrap();
		verify_monotonic_progress(&captured, 0.1);
		verify_progress_finished(&captured);

		// Level 4 has 256 tiles
		// For Push/Pop: tn_read = 256, tn_write = 256
		// total = midpoint(256, 256) = 256
		let last = captured.last().unwrap();
		assert_eq!(last.total, 256, "Total should be 256 for level 4 Push/Pop path");

		Ok(())
	}
}
