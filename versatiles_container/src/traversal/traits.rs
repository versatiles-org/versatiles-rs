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

			use TraversalTranslationStep::*;

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
