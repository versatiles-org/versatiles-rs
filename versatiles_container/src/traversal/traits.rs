//! Extension trait for tile source traversal.
//!
//! This module provides [`TileSourceTraverseExt`], an extension trait that adds
//! traversal capabilities to any [`TileSource`] implementation.

use super::{Traversal, TraversalTranslationStep, translate_traversals};
use crate::{ProgressHandle, Tile, TileSource, TilesRuntime, TraversalCache};
use anyhow::Result;
use futures::{StreamExt, future::BoxFuture, stream};
use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};
use versatiles_core::{TileBBox, TileCoord, TileStream};

/// Helper for consistent progress tracking using midpoint of read/write operations.
struct ProgressTracker {
	progress: ProgressHandle,
	ti_read: AtomicU64,
	ti_write: AtomicU64,
	ti_offset: AtomicU64,
}

impl ProgressTracker {
	fn new(progress: ProgressHandle) -> Self {
		Self {
			progress,
			ti_read: AtomicU64::new(0),
			ti_write: AtomicU64::new(0),
			ti_offset: AtomicU64::new(0),
		}
	}

	fn inc(&self, count: u64) {
		self.ti_offset.fetch_add(count, Ordering::Relaxed);
		self.update_position();
	}

	fn inc_read(&self, value: u64) {
		self.ti_read.fetch_add(value, Ordering::Relaxed);
		self.ti_offset.store(0, Ordering::Relaxed);
		self.update_position();
	}

	fn inc_write(&self, value: u64) {
		self.ti_write.fetch_add(value, Ordering::Relaxed);
		self.ti_offset.store(0, Ordering::Relaxed);
		self.update_position();
	}

	fn inc_read_write(&self, read: u64, write: u64) {
		self.ti_read.fetch_add(read, Ordering::Relaxed);
		self.ti_write.fetch_add(write, Ordering::Relaxed);
		self.ti_offset.store(0, Ordering::Relaxed);
		self.update_position();
	}

	fn update_position(&self) {
		let read = self.ti_read.load(Ordering::Relaxed);
		let write = self.ti_write.load(Ordering::Relaxed);
		let offset = self.ti_offset.load(Ordering::Relaxed);
		self.progress.set_position((read + write + offset) / 2);
	}

	fn finish(&self) {
		self.update_position();
		self.progress.finish();
	}
}

/// Extension trait providing traversal with higher-rank trait bounds (HRTBs).
///
/// This trait is separate from [`TileSource`] to maintain object safety while
/// still supporting complex traversal scenarios that require HRTBs.
///
/// Automatically implemented for all types that implement [`TileSource`].
pub trait TileSourceTraverseExt: TileSource {
	/// Traverses all tiles according to a traversal plan, invoking a callback for each batch.
	///
	/// This method translates between the source's preferred traversal order and the desired
	/// write/consumption order, handling caching for `Push/Pop` phases as needed.
	///
	/// # Arguments
	///
	/// * `traversal_write` - Desired traversal order for consumption
	/// * `callback` - Async function called for each (bbox, stream) pair
	/// * `runtime` - Runtime configuration for caching and progress tracking
	/// * `progress_message` - Optional progress bar label
	fn traverse_all_tiles<'s, 'a, C>(
		&'s self,
		traversal_write: &'s Traversal,
		mut callback: C,
		runtime: TilesRuntime,
		progress_message: Option<&str>,
	) -> impl core::future::Future<Output = Result<()>> + Send + 'a
	where
		C: FnMut(TileBBox, TileStream<'a, Tile>) -> BoxFuture<'a, Result<()>> + Send + 'a,
		's: 'a,
	{
		let progress_message = progress_message.unwrap_or("processing tiles").to_string();

		async move {
			let traversal_steps = translate_traversals(
				&self.metadata().bbox_pyramid,
				&self.metadata().traversal,
				traversal_write,
			)?;

			use TraversalTranslationStep::{Pop, Push, Stream};

			let mut tn_read = 0;
			let mut tn_write = 0;

			for step in &traversal_steps {
				match step {
					Push(bboxes_in, _) => {
						tn_read += bboxes_in.iter().map(TileBBox::count_tiles).sum::<u64>();
					}
					Pop(_, bbox_out) => {
						tn_write += bbox_out.count_tiles();
					}
					Stream(bboxes_in, bbox_out) => {
						tn_read += bboxes_in.iter().map(TileBBox::count_tiles).sum::<u64>();
						tn_write += bbox_out.count_tiles();
					}
				}
			}
			let progress = runtime.create_progress(&progress_message, u64::midpoint(tn_read, tn_write));
			let tracker = Arc::new(ProgressTracker::new(progress));

			let cache = Arc::new(TraversalCache::<(TileCoord, Tile)>::new(runtime.cache_type()));
			for step in traversal_steps {
				match step {
					Push(bboxes, index) => {
						log::trace!("Cache {bboxes:?} at index {index}");
						let limits = versatiles_core::ConcurrencyLimits::default();
						let read_operations = bboxes.iter().map(TileBBox::count_tiles).sum::<u64>();
						stream::iter(bboxes)
							.map(|bbox| {
								let tracker = tracker.clone();
								let c = cache.clone();
								async move {
									let vec = self
										.get_tile_stream(bbox)
										.await?
										.inspect(move || tracker.inc(1))
										.to_vec()
										.await;

									c.append(index, vec)?;

									Ok::<_, anyhow::Error>(())
								}
							})
							.buffer_unordered(limits.io_bound) // I/O-bound: reading tiles from disk/network
							.collect::<Vec<_>>()
							.await
							.into_iter()
							.collect::<Result<Vec<_>>>()?;
						tracker.inc_read(read_operations);
					}
					Pop(index, bbox) => {
						log::trace!("Uncache {bbox:?} at index {index}");
						let vec = cache.take(index)?.unwrap();
						let tracker2 = tracker.clone();
						let stream = TileStream::from_vec(vec).inspect(move || tracker2.inc(1));
						callback(bbox, stream).await?;
						tracker.inc_write(bbox.count_tiles());
					}
					Stream(bboxes, bbox) => {
						log::trace!("Stream {bbox:?}");
						let tracker2 = tracker.clone();
						let read_operations = bboxes.iter().map(TileBBox::count_tiles).sum::<u64>();
						let streams = stream::iter(bboxes).map(move |bbox| {
							let tracker = tracker2.clone();
							async move {
								self.get_tile_stream(bbox).await.unwrap().inspect(move || {
									tracker.inc(2);
								})
							}
						});
						callback(bbox, TileStream::from_streams(streams)).await?;
						tracker.inc_read_write(read_operations, bbox.count_tiles());
					}
				}
			}

			tracker.finish();
			Ok(())
		}
	}
}

// Blanket implementation: all TileSource implementors get traversal support
impl<T: TileSource + ?Sized> TileSourceTraverseExt for T {}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
mod tests {
	use super::*;
	use crate::{SourceType, TileSourceMetadata, TilesRuntime, TraversalOrder};
	use anyhow::Result;
	use async_trait::async_trait;
	use rstest::rstest;
	use std::sync::atomic::{AtomicU64, Ordering};
	use versatiles_core::{Blob, TileBBoxPyramid, TileCompression, TileFormat, TileJSON};

	/// Test reader that produces tiles with predictable content.
	#[derive(Debug)]
	struct TestReader {
		metadata: TileSourceMetadata,
		tilejson: TileJSON,
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
			}
		}
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
			Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |coord| {
				// Create tile with coord encoded in data for verification
				let data = format!("tile:{},{},{}", coord.level, coord.x, coord.y);
				Some(Tile::from_blob(Blob::from(data), compression, format))
			}))
		}
	}

	#[tokio::test]
	async fn test_traverse_streaming_mode() -> Result<()> {
		// When source and write traversals are both ANY, tiles are streamed directly
		let reader = TestReader::new(Traversal::ANY, 2);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let tile_count = Arc::new(AtomicU64::new(0));
		let count_clone = tile_count.clone();

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				|_bbox, stream| {
					let count = count_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						count.fetch_add(tiles.len() as u64, Ordering::SeqCst);
						Ok(())
					})
				},
				runtime,
				Some("test streaming"),
			)
			.await?;

		// Level 0: 1, Level 1: 4, Level 2: 16 = 21 tiles
		assert_eq!(tile_count.load(Ordering::SeqCst), 21);
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_with_reordering() -> Result<()> {
		// DepthFirst source traversal to AnyOrder write traversal
		// This tests the traversal translation without needing caching
		let source_traversal = Traversal::new(super::super::TraversalOrder::DepthFirst, 1, 256)?;
		let write_traversal = Traversal::new(super::super::TraversalOrder::AnyOrder, 1, 256)?;

		let reader = TestReader::new(source_traversal, 2);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let tile_count = Arc::new(AtomicU64::new(0));
		let count_clone = tile_count.clone();

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					let count = count_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						count.fetch_add(tiles.len() as u64, Ordering::SeqCst);
						Ok(())
					})
				},
				runtime,
				None, // Test default progress message
			)
			.await?;

		// Levels 0-2: 1 + 4 + 16 = 21 tiles
		assert_eq!(tile_count.load(Ordering::SeqCst), 21);
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_with_disk_cache() -> Result<()> {
		// Test traversal with disk-backed cache
		let source_traversal = Traversal::new(super::super::TraversalOrder::DepthFirst, 1, 256)?;
		let write_traversal = Traversal::new(super::super::TraversalOrder::AnyOrder, 1, 256)?;

		let reader = TestReader::new(source_traversal, 2);
		let temp_dir = tempfile::TempDir::new()?;
		let runtime = TilesRuntime::builder().with_disk_cache(temp_dir.path()).build();

		let tile_count = Arc::new(AtomicU64::new(0));
		let count_clone = tile_count.clone();

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					let count = count_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						count.fetch_add(tiles.len() as u64, Ordering::SeqCst);
						Ok(())
					})
				},
				runtime,
				Some("disk cache test"),
			)
			.await?;

		// Levels 0-2: 1 + 4 + 16 = 21 tiles
		assert_eq!(tile_count.load(Ordering::SeqCst), 21);
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_verifies_tile_content() -> Result<()> {
		let reader = TestReader::new(Traversal::ANY, 1);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let received_coords = Arc::new(std::sync::Mutex::new(Vec::new()));
		let coords_clone = received_coords.clone();

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				|_bbox, stream| {
					let coords = coords_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						for (coord, tile) in tiles {
							// Verify tile data contains expected coord
							let blob = tile.into_blob(TileCompression::Uncompressed)?;
							let data = String::from_utf8_lossy(blob.as_slice());
							let expected = format!("tile:{},{},{}", coord.level, coord.x, coord.y);
							assert_eq!(data, expected);
							coords.lock().unwrap().push(coord);
						}
						Ok(())
					})
				},
				runtime,
				None,
			)
			.await?;

		let coords = received_coords.lock().unwrap();
		assert_eq!(coords.len(), 5); // 1 + 4 tiles
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_empty_pyramid() -> Result<()> {
		// Test with a very restricted pyramid (level 0 only, single tile)
		let reader = TestReader::new(Traversal::ANY, 0);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let tile_count = Arc::new(AtomicU64::new(0));
		let count_clone = tile_count.clone();

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				|_bbox, stream| {
					let count = count_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						count.fetch_add(tiles.len() as u64, Ordering::SeqCst);
						Ok(())
					})
				},
				runtime,
				None,
			)
			.await?;

		assert_eq!(tile_count.load(Ordering::SeqCst), 1);
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_callback_receives_correct_bbox() -> Result<()> {
		let reader = TestReader::new(Traversal::ANY, 1);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let bboxes_received = Arc::new(std::sync::Mutex::new(Vec::new()));
		let bboxes_clone = bboxes_received.clone();

		reader
			.traverse_all_tiles(
				&Traversal::ANY,
				|bbox, stream| {
					let bboxes = bboxes_clone.clone();
					Box::pin(async move {
						bboxes.lock().unwrap().push(bbox);
						stream.drain_and_count().await;
						Ok(())
					})
				},
				runtime,
				None,
			)
			.await?;

		let bboxes = bboxes_received.lock().unwrap();
		// Should have received bboxes for levels 0 and 1
		assert!(!bboxes.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_push_pop_caching_path() -> Result<()> {
		// This test exercises the Push/Pop caching path by using:
		// - Read traversal: DepthFirst with small max_size (8)
		// - Write traversal: AnyOrder with larger min_size (16)
		// This forces tiles to be cached (Push) and then retrieved (Pop)
		let source_traversal = Traversal::new(TraversalOrder::DepthFirst, 1, 8)?;
		let write_traversal = Traversal::new(TraversalOrder::AnyOrder, 16, 16)?;

		// Use zoom levels 4-6 (need at least level 4 for 16x16 bboxes)
		let reader = TestReader::with_level_range(source_traversal, 4, 6);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let tile_count = Arc::new(AtomicU64::new(0));
		let count_clone = tile_count.clone();

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					let count = count_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						count.fetch_add(tiles.len() as u64, Ordering::SeqCst);
						Ok(())
					})
				},
				runtime,
				Some("push/pop cache test"),
			)
			.await?;

		// Levels 4-6: 256 + 1024 + 4096 = 5376 tiles
		assert_eq!(tile_count.load(Ordering::SeqCst), 5376);
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_push_pop_with_disk_cache() -> Result<()> {
		// Test Push/Pop path with disk-backed cache
		let source_traversal = Traversal::new(TraversalOrder::DepthFirst, 1, 8)?;
		let write_traversal = Traversal::new(TraversalOrder::AnyOrder, 16, 16)?;

		// Use zoom levels 4-5 (need at least level 4 for 16x16 bboxes)
		let reader = TestReader::with_level_range(source_traversal, 4, 5);
		let temp_dir = tempfile::TempDir::new()?;
		let runtime = TilesRuntime::builder().with_disk_cache(temp_dir.path()).build();

		let tile_count = Arc::new(AtomicU64::new(0));
		let count_clone = tile_count.clone();

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					let count = count_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						count.fetch_add(tiles.len() as u64, Ordering::SeqCst);
						Ok(())
					})
				},
				runtime,
				Some("push/pop disk cache test"),
			)
			.await?;

		// Levels 4-5: 256 + 1024 = 1280 tiles
		assert_eq!(tile_count.load(Ordering::SeqCst), 1280);
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_push_pop_verifies_tile_content() -> Result<()> {
		// Verify tiles retain correct content through Push/Pop caching
		let source_traversal = Traversal::new(TraversalOrder::DepthFirst, 1, 8)?;
		let write_traversal = Traversal::new(TraversalOrder::AnyOrder, 16, 16)?;

		// Use zoom levels 4-5 (need at least level 4 for 16x16 bboxes)
		let reader = TestReader::with_level_range(source_traversal, 4, 5);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let received_coords = Arc::new(std::sync::Mutex::new(Vec::new()));
		let coords_clone = received_coords.clone();

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					let coords = coords_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						for (coord, tile) in tiles {
							// Verify tile data contains expected coord after caching
							let blob = tile.into_blob(TileCompression::Uncompressed)?;
							let data = String::from_utf8_lossy(blob.as_slice());
							let expected = format!("tile:{},{},{}", coord.level, coord.x, coord.y);
							assert_eq!(data, expected, "Tile content mismatch after cache");
							coords.lock().unwrap().push(coord);
						}
						Ok(())
					})
				},
				runtime,
				None,
			)
			.await?;

		let coords = received_coords.lock().unwrap();
		// Levels 4-5: 256 + 1024 = 1280 tiles
		assert_eq!(coords.len(), 1280);
		Ok(())
	}

	#[tokio::test]
	async fn test_traverse_pmtiles_order_to_any() -> Result<()> {
		// Test PMTiles traversal order with Push/Pop
		let source_traversal = Traversal::new(TraversalOrder::PMTiles, 1, 8)?;
		let write_traversal = Traversal::new(TraversalOrder::AnyOrder, 16, 16)?;

		// Use zoom levels 4-5 (need at least level 4 for 16x16 bboxes)
		let reader = TestReader::with_level_range(source_traversal, 4, 5);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let tile_count = Arc::new(AtomicU64::new(0));
		let count_clone = tile_count.clone();

		reader
			.traverse_all_tiles(
				&write_traversal,
				|_bbox, stream| {
					let count = count_clone.clone();
					Box::pin(async move {
						let tiles = stream.to_vec().await;
						count.fetch_add(tiles.len() as u64, Ordering::SeqCst);
						Ok(())
					})
				},
				runtime,
				Some("pmtiles to any"),
			)
			.await?;

		// Levels 4-5: 256 + 1024 = 1280 tiles
		assert_eq!(tile_count.load(Ordering::SeqCst), 1280);
		Ok(())
	}

	/// Test that verifies the callback receives bboxes in the correct write traversal order.
	/// Uses rstest to test multiple combinations of source and write traversal orders.
	///
	/// Note: Only compatible traversal combinations are tested. Converting between
	/// incompatible orders (e.g., DepthFirst to PMTiles) is not supported by the
	/// traversal translation system.
	#[rstest]
	#[case::any_to_depthfirst(TraversalOrder::AnyOrder, 1, 256, TraversalOrder::DepthFirst, 1, 256, 2)]
	#[case::any_to_pmtiles(TraversalOrder::AnyOrder, 1, 256, TraversalOrder::PMTiles, 1, 256, 2)]
	#[case::depthfirst_to_depthfirst(TraversalOrder::DepthFirst, 1, 256, TraversalOrder::DepthFirst, 1, 256, 3)]
	#[case::pmtiles_to_pmtiles(TraversalOrder::PMTiles, 1, 256, TraversalOrder::PMTiles, 1, 256, 3)]
	#[case::depthfirst_to_any(TraversalOrder::DepthFirst, 1, 64, TraversalOrder::AnyOrder, 1, 64, 3)]
	#[case::pmtiles_to_any(TraversalOrder::PMTiles, 1, 64, TraversalOrder::AnyOrder, 1, 64, 3)]
	#[tokio::test]
	async fn test_traverse_order_verification(
		#[case] source_order: TraversalOrder,
		#[case] source_min_size: u32,
		#[case] source_max_size: u32,
		#[case] write_order: TraversalOrder,
		#[case] write_min_size: u32,
		#[case] write_max_size: u32,
		#[case] max_level: u8,
	) -> Result<()> {
		let source_traversal = Traversal::new(source_order, source_min_size, source_max_size)?;
		let write_traversal = Traversal::new(write_order, write_min_size, write_max_size)?;

		let reader = TestReader::new(source_traversal, max_level);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let bboxes_received = Arc::new(std::sync::Mutex::new(Vec::new()));
		let bboxes_clone = bboxes_received.clone();

		reader
			.traverse_all_tiles(
				&write_traversal,
				|bbox, stream| {
					let bboxes = bboxes_clone.clone();
					Box::pin(async move {
						bboxes.lock().unwrap().push(bbox);
						stream.drain_and_count().await;
						Ok(())
					})
				},
				runtime,
				None,
			)
			.await?;

		let bboxes = bboxes_received.lock().unwrap();

		// Verify we received some bboxes
		assert!(!bboxes.is_empty(), "Should receive at least one bbox");

		// Verify the order matches the write traversal order
		let write_size = write_traversal.max_size()?;
		assert!(
			write_order.verify_order(&bboxes, write_size),
			"Bboxes should be in {write_order:?} order, but received: {:?}",
			bboxes.iter().take(10).collect::<Vec<_>>()
		);

		Ok(())
	}

	/// Test order verification with Push/Pop caching path
	/// When read max_size < write min_size, the Push/Pop path is used
	#[rstest]
	#[case::depthfirst_cached_to_any(TraversalOrder::DepthFirst, 1, 8, TraversalOrder::AnyOrder, 16, 16, 4, 5)]
	#[case::pmtiles_cached_to_any(TraversalOrder::PMTiles, 1, 8, TraversalOrder::AnyOrder, 16, 16, 4, 5)]
	#[tokio::test]
	async fn test_traverse_order_with_push_pop_caching(
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

		// Use level range to ensure write bbox sizes fit
		let reader = TestReader::with_level_range(source_traversal, min_level, max_level);
		let runtime = TilesRuntime::builder().with_memory_cache().build();

		let bboxes_received = Arc::new(std::sync::Mutex::new(Vec::new()));
		let bboxes_clone = bboxes_received.clone();

		reader
			.traverse_all_tiles(
				&write_traversal,
				|bbox, stream| {
					let bboxes = bboxes_clone.clone();
					Box::pin(async move {
						bboxes.lock().unwrap().push(bbox);
						stream.drain_and_count().await;
						Ok(())
					})
				},
				runtime,
				None,
			)
			.await?;

		let bboxes = bboxes_received.lock().unwrap();

		// Verify we received some bboxes
		assert!(!bboxes.is_empty(), "Should receive at least one bbox");

		// For AnyOrder write traversal, any order is valid
		// For other orders, verify the specific order
		if write_order != TraversalOrder::AnyOrder {
			let write_size = write_traversal.max_size()?;
			assert!(
				write_order.verify_order(&bboxes, write_size),
				"Bboxes should be in {write_order:?} order with Push/Pop caching"
			);
		}

		Ok(())
	}
}
