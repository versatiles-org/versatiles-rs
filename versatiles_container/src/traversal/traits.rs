//! Extension trait for tile source traversal.
//!
//! This module provides [`TileSourceTraverseExt`], an extension trait that adds
//! traversal capabilities to any [`TileSource`] implementation.

use super::{Traversal, TraversalTranslationStep, translate_traversals};
use crate::{Tile, TileSource, TilesRuntime, TraversalCache};
use anyhow::Result;
use futures::{StreamExt, future::BoxFuture, stream};
use std::sync::Arc;
use versatiles_core::{TileBBox, TileCoord, TileStream};

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

			let mut ti_read = 0;
			let mut ti_write = 0;

			let cache = Arc::new(TraversalCache::<(TileCoord, Tile)>::new(runtime.cache_type()));
			for step in traversal_steps {
				match step {
					Push(bboxes, index) => {
						log::trace!("Cache {bboxes:?} at index {index}");
						let limits = versatiles_core::ConcurrencyLimits::default();
						stream::iter(bboxes.clone())
							.map(|bbox| {
								let progress = progress.clone();
								let c = cache.clone();
								async move {
									let vec = self
										.get_tile_stream(bbox)
										.await?
										.inspect(move || progress.inc(1))
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
						ti_read += bboxes.iter().map(TileBBox::count_tiles).sum::<u64>();
					}
					Pop(index, bbox) => {
						log::trace!("Uncache {bbox:?} at index {index}");
						let vec = cache.take(index)?.unwrap();
						let progress = progress.clone();
						let stream = TileStream::from_vec(vec).inspect(move || progress.inc(1));
						callback(bbox, stream).await?;
						ti_write += bbox.count_tiles();
					}
					Stream(bboxes, bbox) => {
						log::trace!("Stream {bbox:?}");
						let progress = progress.clone();
						let streams = stream::iter(bboxes.clone()).map(move |bbox| {
							let progress = progress.clone();
							async move {
								self
									.get_tile_stream(bbox)
									.await
									.unwrap()
									.inspect(move || progress.inc(2))
							}
						});
						callback(bbox, TileStream::from_streams(streams)).await?;
						ti_read += bboxes.iter().map(TileBBox::count_tiles).sum::<u64>();
						ti_write += bbox.count_tiles();
					}
				}
				progress.set_position(u64::midpoint(ti_read, ti_write));
			}

			progress.finish();
			Ok(())
		}
	}
}

// Blanket implementation: all TileSource implementors get traversal support
impl<T: TileSource + ?Sized> TileSourceTraverseExt for T {}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{SourceType, TileSourceMetadata, TilesRuntime};
	use anyhow::Result;
	use async_trait::async_trait;
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

		async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
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
}
