//! Unified interface for tile sources (readers and processors).
//!
//! This module defines [`TileSourceTrait`], the common interface for anything that produces tiles,
//! whether reading from physical containers or processing/transforming tiles from upstream sources.
//!
//! ## Design Philosophy
//!
//! Both container readers (e.g., MBTiles, VersaTiles) and tile processors (e.g., filters, format converters)
//! share the same fundamental operations:
//! - Provide metadata (TileJSON, parameters)
//! - Stream tiles from a bounding box
//! - Support runtime composition
//!
//! By unifying these under a single trait, we enable:
//! - Seamless composition of readers and processors
//! - Type-safe pipeline construction
//! - Clear separation between data sources and transformations

use crate::{
	CacheMap, SourceType, Tile, TileSourceMetadata, TilesRuntime,
	traversal::{Traversal, TraversalTranslationStep, translate_traversals},
};
use anyhow::Result;
use async_trait::async_trait;
use futures::{StreamExt, future::BoxFuture, stream};
use std::{fmt::Debug, sync::Arc};
#[cfg(feature = "cli")]
use versatiles_core::{ProbeDepth, utils::PrettyPrint};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TileStream};

/// Unified object-safe interface for reading or processing tiles.
///
/// Implementors include:
/// * **Container readers**: Read tiles from physical storage (files, databases, archives)
/// * **Tile processors**: Transform tiles from upstream sources (filtering, format conversion, etc.)
/// * **Composite sources**: Combine multiple sources (stacking, merging)
///
/// The trait is object-safe to support dynamic dispatch via `Box<dyn TileSourceTrait>`,
/// enabling runtime composition of heterogeneous sources and processors.
///
/// ## Object Safety & Extension Traits
///
/// For operations requiring higher-rank trait bounds (HRTBs), see [`TileSourceTraverseExt`],
/// which provides advanced traversal while keeping the base trait object-safe.
#[async_trait]
pub trait TileSourceTrait: Debug + Send + Sync + Unpin {
	/// Returns the source type (container format, processor name, or composite).
	///
	/// This helps distinguish between:
	/// - Physical containers: `SourceType::Container("mbtiles")`
	/// - Tile processors: `SourceType::Processor("filter")`
	/// - Composite sources: `SourceType::Composite`
	fn source_type(&self) -> Arc<SourceType>;

	/// Returns runtime parameters describing the tiles this source will produce.
	///
	/// Includes:
	/// - `bbox_pyramid`: Spatial extent at each zoom level
	/// - `tile_compression`: Current output compression
	/// - `tile_format`: Tile format (PNG, JPG, MVT, etc.)
	fn parameters(&self) -> &TileSourceMetadata;

	/// Returns the TileJSON metadata for this tileset.
	fn tilejson(&self) -> &TileJSON;

	/// Returns the preferred traversal order hint (default: [`Traversal::ANY`]).
	///
	/// Sources that can efficiently stream in a specific order should override this.
	fn traversal(&self) -> &Traversal {
		&Traversal::ANY
	}

	/// Fetches a single tile at the given coordinate.
	///
	/// Returns:
	/// - `Ok(Some(tile))` if the tile exists
	/// - `Ok(None)` for gaps or empty tiles
	/// - `Err(_)` on read/processing errors
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let bbox = coord.to_tile_bbox();
		let mut stream = self.get_tile_stream(bbox).await?;
		Ok(stream.next().await.map(|(_, t)| t))
	}

	/// Asynchronously streams all tiles within the given bounding box.
	///
	/// Returns a [`TileStream`] of `(TileCoord, Tile)` pairs. The stream handles
	/// backpressure and supports concurrent pulls.
	///
	/// Default implementation wraps individual `get_tile` calls with internal synchronization.
	/// Sources that can optimize bulk reads should override this.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>>;

	async fn stream_individual_tiles(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		Ok(TileStream::from_coord_vec_async(
			bbox.into_iter_coords().collect(),
			async move |c| self.get_tile(&c).await.ok().flatten().map(|t| (c, t)),
		))
	}

	/// Performs a hierarchical CLI probe at the specified depth.
	///
	/// Probes metadata, parameters, container specifics, tiles, and tile contents
	/// based on the requested depth level.
	#[cfg(feature = "cli")]
	async fn probe(&self, level: ProbeDepth) -> Result<()> {
		use ProbeDepth::*;

		let mut print = PrettyPrint::new();

		let cat = print.get_category("meta_data").await;
		cat.add_key_value("source_type", &self.source_type().to_string()).await;

		cat.add_key_json("meta", &self.tilejson().as_json_value()).await;

		self
			.probe_parameters(&mut print.get_category("parameters").await)
			.await?;

		if matches!(level, Container | Tiles | TileContents) {
			log::debug!("probing source {:?} at depth {:?}", self.source_type(), level);
			self.probe_container(&print.get_category("container").await).await?;
		}

		if matches!(level, Tiles | TileContents) {
			log::debug!(
				"probing tiles {:?} at depth {:?}",
				self.tilejson().as_json_value(),
				level
			);
			self.probe_tiles(&print.get_category("tiles").await).await?;
		}

		if matches!(level, TileContents) {
			log::debug!(
				"probing tile contents {:?} at depth {:?}",
				self.tilejson().as_json_value(),
				level
			);
			self
				.probe_tile_contents(&print.get_category("tile contents").await)
				.await?;
		}

		Ok(())
	}

	/// Writes source parameters (bbox pyramid, formats, compression) to the CLI reporter.
	#[cfg(feature = "cli")]
	async fn probe_parameters(&self, print: &mut PrettyPrint) -> Result<()> {
		let parameters = self.parameters();
		let p = print.get_list("bbox_pyramid").await;
		for level in parameters.bbox_pyramid.iter_levels() {
			p.add_value(level).await;
		}
		print
			.add_key_value("bbox", &format!("{:?}", parameters.bbox_pyramid.get_geo_bbox()))
			.await;
		print
			.add_key_value("tile compression", &parameters.tile_compression)
			.await;
		print.add_key_value("tile format", &parameters.tile_format).await;
		Ok(())
	}

	/// Writes source-specific metadata or a placeholder if not implemented.
	///
	/// Container readers may override to provide format-specific details.
	#[cfg(feature = "cli")]
	async fn probe_container(&self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this source")
			.await;
		Ok(())
	}

	/// Writes tile-level probing output or a placeholder if not implemented.
	#[cfg(feature = "cli")]
	async fn probe_tiles(&self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tiles probing is not implemented for this source")
			.await;
		Ok(())
	}

	/// Writes sample tile content diagnostics or a placeholder if not implemented.
	#[cfg(feature = "cli")]
	async fn probe_tile_contents(&self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tile contents probing is not implemented for this source")
			.await;
		Ok(())
	}

	/// Converts `self` into a boxed trait object for dynamic dispatch.
	fn boxed(self) -> Box<dyn TileSourceTrait>
	where
		Self: Sized + 'static,
	{
		Box::new(self)
	}
}

/// Extension trait providing traversal with higher-rank trait bounds (HRTBs).
///
/// This trait is separate from [`TileSourceTrait`] to maintain object safety while
/// still supporting complex traversal scenarios that require HRTBs.
///
/// Automatically implemented for all types that implement [`TileSourceTrait`].
pub trait TileSourceTraverseExt: TileSourceTrait {
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
			let traversal_steps =
				translate_traversals(&self.parameters().bbox_pyramid, self.traversal(), traversal_write)?;

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

			let cache = Arc::new(CacheMap::<usize, (TileCoord, Tile)>::new(runtime.cache_type()));
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

									c.append(&index, vec)?;

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
						let vec = cache.remove(&index)?.unwrap();
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

// Blanket implementation: all TileSourceTrait implementors get traversal support
impl<T: TileSourceTrait + ?Sized> TileSourceTraverseExt for T {}

/// Tests cover trait defaults, parameter plumbing, streaming behavior, and the CLI probe stubs.
#[cfg(test)]
mod tests {
	#[cfg(feature = "cli")]
	use super::ProbeDepth;
	use super::*;
	#[cfg(feature = "cli")]
	use versatiles_core::utils::PrettyPrint;
	use versatiles_core::{Blob, TileBBoxPyramid, TileCompression, TileFormat};

	#[derive(Debug)]
	struct TestReader {
		parameters: TileSourceMetadata,
		tilejson: TileJSON,
	}

	impl TestReader {
		fn new_dummy() -> TestReader {
			let mut tilejson = TileJSON::default();
			tilejson.set_string("metadata", "test").unwrap();
			TestReader {
				parameters: TileSourceMetadata {
					bbox_pyramid: TileBBoxPyramid::new_full(3),
					tile_compression: TileCompression::Gzip,
					tile_format: TileFormat::MVT,
				},
				tilejson,
			}
		}
	}

	#[async_trait]
	impl TileSourceTrait for TestReader {
		fn source_type(&self) -> Arc<SourceType> {
			SourceType::new_container("dummy_format", "dummy_uri")
		}

		fn parameters(&self) -> &TileSourceMetadata {
			&self.parameters
		}

		fn tilejson(&self) -> &TileJSON {
			&self.tilejson
		}

		async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
			let tile_compression = self.parameters.tile_compression;
			let tile_format = self.parameters.tile_format;
			Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_| {
				Some(Tile::from_blob(
					Blob::from("test tile data"),
					tile_compression,
					tile_format,
				))
			}))
		}
	}

	#[tokio::test]
	async fn test_parameters() {
		let reader = TestReader::new_dummy();
		let parameters = reader.parameters();
		assert_eq!(parameters.tile_compression, TileCompression::Gzip);
		assert_eq!(parameters.tile_format, TileFormat::MVT);
		assert_eq!(parameters.bbox_pyramid.get_level_min().unwrap(), 0);
		assert_eq!(parameters.bbox_pyramid.get_level_max().unwrap(), 3);
	}

	#[tokio::test]
	async fn test_get_meta() -> Result<()> {
		let reader = TestReader::new_dummy();
		assert_eq!(
			reader.tilejson().as_string(),
			"{\"metadata\":\"test\",\"tilejson\":\"3.0.0\"}"
		);
		Ok(())
	}

	#[tokio::test]
	async fn test_get_tile_stream() -> Result<()> {
		let reader = TestReader::new_dummy();
		let bbox = TileBBox::from_min_and_max(1, 0, 0, 1, 1)?;
		let stream = reader.get_tile_stream(bbox).await?;

		assert_eq!(stream.drain_and_count().await, 4); // Assuming 4 tiles in a 2x2 bbox
		Ok(())
	}

	#[tokio::test]
	async fn test_probe_tile_contents() -> Result<()> {
		#[cfg(feature = "cli")]
		{
			use versatiles_core::utils::PrettyPrint;

			let reader = TestReader::new_dummy();
			let mut print = PrettyPrint::new();
			reader
				.probe_tile_contents(&print.get_category("tile contents").await)
				.await?;
		}
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_parameters() -> Result<()> {
		let reader = TestReader::new_dummy();
		let mut print = PrettyPrint::new();
		reader.probe_parameters(&mut print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_container() -> Result<()> {
		let reader = TestReader::new_dummy();
		let print = PrettyPrint::new();
		reader.probe_container(&print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_tiles() -> Result<()> {
		let reader = TestReader::new_dummy();
		let print = PrettyPrint::new();
		reader.probe_tiles(&print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_all_levels() -> Result<()> {
		let reader = TestReader::new_dummy();
		reader.probe(ProbeDepth::Container).await?;
		reader.probe(ProbeDepth::Tiles).await?;
		reader.probe(ProbeDepth::TileContents).await?;
		Ok(())
	}
}
