//! Read and probe tile data from various container formats in a streaming-friendly way.
//!
//! This module defines the object‑safe [`TilesReaderTrait`], which exposes:
//! - Lightweight metadata access (`source_name`, `container_name`, [`tilejson`])
//! - Runtime parameters and formats via [`parameters`]
//! - Random access to individual tiles (`get_tile`)
//! - Async streaming over regions via [`get_tile_stream`]
//! - An optional CLI probing interface (behind the `cli` feature)
//!
//! ### Object safety & adapters
//! The trait is intentionally object‑safe so readers can be stored behind `Box<dyn TilesReaderTrait>`
//! and composed at runtime (e.g., converter/filters). For traversal that requires higher‑rank trait bounds
//! (HRTBs), see [`TilesReaderTraverseExt`], which keeps the base trait object‑safe.
//!
//! ### Example: stream tiles from a bbox
//! ```rust
//! # use versatiles_container::*;
//! # use versatiles_core::*;
//! # async fn demo() -> anyhow::Result<()> {
//! let registry = ContainerRegistry::default();
//! let reader = registry.get_reader_from_str("../testdata/berlin.mbtiles").await?;
//! let bbox = TileBBox::from_min_and_max(1, 0, 0, 1, 1)?;
//! let mut stream = reader.get_tile_stream(bbox).await?;
//! // drain tiles
//! let count = stream.drain_and_count().await;
//! # assert!(count > 0);
//! # Ok(())
//! # }
//! ```

use crate::{CacheMap, ProcessingConfig, Tile};
use anyhow::Result;
use async_trait::async_trait;
use futures::{StreamExt, future::BoxFuture, stream};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
#[cfg(feature = "cli")]
use versatiles_core::{ProbeDepth, utils::PrettyPrint};
use versatiles_core::{
	TileBBox, TileCompression, TileCoord, TileJSON, TileStream, TilesReaderParameters, Traversal,
	TraversalTranslationStep, progress::get_progress_bar, translate_traversals,
};

/// Object‑safe interface for reading tiles from a container.
///
/// Implementors provide access to:
/// * **Identification**: human‑readable source and container names.
/// * **Metadata**: [`TileJSON`] and runtime [`TilesReaderParameters`].
/// * **Access patterns**: single‑tile fetches and async streaming over a [`TileBBox`].
/// * **Traversal hint**: override [`TilesReaderTrait::traversal`] to advertise a preferred read order; the default is [`Traversal::ANY`].
///
/// The trait remains object‑safe to support dynamic dispatch and runtime composition.
#[async_trait]
pub trait TilesReaderTrait: Debug + Send + Sync + Unpin {
	/// Returns a short, human‑readable identifier for the source (e.g., filename or URI).
	fn source_name(&self) -> &str;

	/// Returns the container type (e.g., `"mbtiles"`, `"pmtiles"`, `"versatiles"`, `"tar"`, `"dir"`).
	fn container_name(&self) -> &str;

	/// Returns runtime reader parameters (bbox pyramid, compression, tile format).
	///
	/// These values describe what the reader **will** return (e.g., the current compression).
	fn parameters(&self) -> &TilesReaderParameters;

	/// Overrides the output compression for subsequent reads.
	///
	/// Implementors should update their internal parameters so [`TilesReaderTrait::parameters`].`tile_compression`
	/// reflects the new setting.
	fn override_compression(&mut self, tile_compression: TileCompression);

	/// Returns the immutable [`TileJSON`] metadata for this set.
	fn tilejson(&self) -> &TileJSON;

	/// Returns the supported/preferred traversal order (default: [`Traversal::ANY`]).
	///
	/// Override in readers that can more efficiently stream in a specific order.
	fn traversal(&self) -> &Traversal {
		&Traversal::ANY
	}

	/// Fetches a single tile at `coord`.
	///
	/// Returns `Ok(Some(tile))` if present, `Ok(None)` for gaps/empty tiles, and `Err(_)` on read errors.
	/// The tile's compression/format follow the current [`TilesReaderTrait::parameters`].
	async fn get_tile(&self, coord: &TileCoord) -> Result<Option<Tile>>;

	/// Asynchronously streams all tiles within `bbox` as `(TileCoord, Tile)` pairs.
	///
	/// Implemented with internal synchronization to allow concurrent pulls from the stream.
	/// Backpressure is handled by the returned [`TileStream`].
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord> = bbox.iter_coords().collect();
		Ok(TileStream::from_coord_vec_async(coords, move |coord| {
			let mutex = mutex.clone();
			async move {
				mutex
					.lock()
					.await
					.get_tile(&coord)
					.await
					.map(|blob_option| blob_option.map(|blob| (coord, blob)))
					.unwrap_or(None)
			}
		}))
	}

	/// Performs a hierarchical CLI probe of metadata, parameters, container, tiles, and contents.
	///
	/// Output is structured using categories/lists for human‑friendly inspection.
	#[cfg(feature = "cli")]
	async fn probe(&mut self, level: ProbeDepth) -> Result<()> {
		use ProbeDepth::*;

		let mut print = PrettyPrint::new();

		let cat = print.get_category("meta_data").await;
		cat.add_key_value("name", self.source_name()).await;
		cat.add_key_value("container", self.container_name()).await;

		cat.add_key_json("meta", &self.tilejson().as_json_value()).await;

		self
			.probe_parameters(&mut print.get_category("parameters").await)
			.await?;

		if matches!(level, Container | Tiles | TileContents) {
			log::debug!("probing container {:?} at depth {:?}", self.container_name(), level);
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

	/// Writes reader parameters (bbox levels, formats, compression) into the CLI reporter.
	#[cfg(feature = "cli")]
	async fn probe_parameters(&mut self, print: &mut PrettyPrint) -> Result<()> {
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

	/// Writes container‑specific metadata or a placeholder warning if not implemented.
	#[cfg(feature = "cli")]
	async fn probe_container(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this container format")
			.await;
		Ok(())
	}

	/// Writes tile‑level probing output or a placeholder warning if not implemented.
	#[cfg(feature = "cli")]
	async fn probe_tiles(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tiles probing is not implemented for this container format")
			.await;
		Ok(())
	}

	/// Writes sample tile content diagnostics or a placeholder warning if not implemented.
	#[cfg(feature = "cli")]
	async fn probe_tile_contents(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tile contents probing is not implemented for this container format")
			.await;
		Ok(())
	}

	/// Converts `self` into a boxed trait object for dynamic dispatch and composition.
	fn boxed(self) -> Box<dyn TilesReaderTrait>
	where
		Self: Sized + 'static,
	{
		Box::new(self)
	}
}

/// Extension trait providing traversal with higher‑rank trait bounds (HRTBs) while
/// keeping [`TilesReaderTrait`] object‑safe.
///
/// Use this when you need to stream tiles across complex traversal plans and hand
/// each produced stream to a callback for further processing.
pub trait TilesReaderTraverseExt: TilesReaderTrait {
	/// Traverses all tiles according to a translated traversal plan and invokes `callback`
	/// for each output [`TileBBox`] with a corresponding [`TileStream`].
	///
	/// * `traversal_write` — desired traversal to write/consume in.
	/// * `callback` — async function to consume each bbox + stream.
	/// * `config` — processing configuration (also used to size caches).
	///
	/// Progress is reported via a progress bar; caching is used to support `Push/Pop` phases.
	fn traverse_all_tiles<'s, 'a, C>(
		&'s self,
		traversal_write: &'s Traversal,
		mut callback: C,
		config: ProcessingConfig,
	) -> impl core::future::Future<Output = Result<()>> + Send + 'a
	where
		C: FnMut(TileBBox, TileStream<'a, Tile>) -> BoxFuture<'a, Result<()>> + Send + 'a,
		's: 'a,
	{
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
			let progress = get_progress_bar("converting tiles", u64::midpoint(tn_read, tn_write));

			let mut ti_read = 0;
			let mut ti_write = 0;

			let cache = Arc::new(Mutex::new(CacheMap::<usize, (TileCoord, Tile)>::new(&config)));
			for step in traversal_steps {
				match step {
					Push(bboxes, index) => {
						log::trace!("Cache {bboxes:?} at index {index}");
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

									let mut cache = c.lock().await;
									cache.append(&index, vec)?;

									Ok::<_, anyhow::Error>(())
								}
							})
							.buffer_unordered(num_cpus::get() / 4)
							.collect::<Vec<_>>()
							.await
							.into_iter()
							.collect::<Result<Vec<_>>>()?;
						ti_read += bboxes.iter().map(TileBBox::count_tiles).sum::<u64>();
					}
					Pop(index, bbox) => {
						log::trace!("Uncache {bbox:?} at index {index}");
						let vec = cache.lock().await.remove(&index)?.unwrap();
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

impl<T: TilesReaderTrait + ?Sized> TilesReaderTraverseExt for T {}

/// Tests cover trait defaults, parameter plumbing, streaming behavior, and the CLI probe stubs.
#[cfg(test)]
mod tests {
	#[cfg(feature = "cli")]
	use super::ProbeDepth;
	use super::*;
	#[cfg(feature = "cli")]
	use versatiles_core::utils::PrettyPrint;
	use versatiles_core::{Blob, TileBBoxPyramid, TileFormat};

	#[derive(Debug)]
	struct TestReader {
		parameters: TilesReaderParameters,
		tilejson: TileJSON,
	}

	impl TestReader {
		fn new_dummy() -> TestReader {
			let mut tilejson = TileJSON::default();
			tilejson.set_string("metadata", "test").unwrap();
			TestReader {
				parameters: TilesReaderParameters {
					bbox_pyramid: TileBBoxPyramid::new_full(3),
					tile_compression: TileCompression::Gzip,
					tile_format: TileFormat::MVT,
				},
				tilejson,
			}
		}
	}

	#[async_trait]
	impl TilesReaderTrait for TestReader {
		fn source_name(&self) -> &'static str {
			"dummy"
		}

		fn container_name(&self) -> &'static str {
			"test container name"
		}

		fn parameters(&self) -> &TilesReaderParameters {
			&self.parameters
		}

		fn override_compression(&mut self, tile_compression: TileCompression) {
			self.parameters.tile_compression = tile_compression;
		}

		fn tilejson(&self) -> &TileJSON {
			&self.tilejson
		}

		async fn get_tile(&self, _coord: &TileCoord) -> Result<Option<Tile>> {
			Ok(Some(Tile::from_blob(
				Blob::from("test tile data"),
				self.parameters.tile_compression,
				self.parameters.tile_format,
			)))
		}
	}

	#[tokio::test]
	async fn test_get_name() {
		let reader = TestReader::new_dummy();
		assert_eq!(reader.source_name(), "dummy");
	}

	#[tokio::test]
	async fn test_container_name() {
		let reader = TestReader::new_dummy();
		assert_eq!(reader.container_name(), "test container name");
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
	async fn test_override_compression() {
		let mut reader = TestReader::new_dummy();
		assert_eq!(reader.parameters().tile_compression, TileCompression::Gzip);

		reader.override_compression(TileCompression::Brotli);
		assert_eq!(reader.parameters().tile_compression, TileCompression::Brotli);
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

			let mut reader = TestReader::new_dummy();
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
		let mut reader = TestReader::new_dummy();
		let mut print = PrettyPrint::new();
		reader.probe_parameters(&mut print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_container() -> Result<()> {
		let mut reader = TestReader::new_dummy();
		let print = PrettyPrint::new();
		reader.probe_container(&print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_tiles() -> Result<()> {
		let mut reader = TestReader::new_dummy();
		let print = PrettyPrint::new();
		reader.probe_tiles(&print).await?;
		Ok(())
	}

	#[cfg(feature = "cli")]
	#[tokio::test]
	async fn test_probe_all_levels() -> Result<()> {
		let mut reader = TestReader::new_dummy();
		reader.probe(ProbeDepth::Container).await?;
		reader.probe(ProbeDepth::Tiles).await?;
		reader.probe(ProbeDepth::TileContents).await?;
		Ok(())
	}

	#[tokio::test]
	async fn test_boxed_trait_object() {
		let reader = TestReader::new_dummy();
		let boxed = reader.boxed();
		// Should forward trait methods
		assert_eq!(boxed.source_name(), "dummy");
		assert_eq!(boxed.container_name(), "test container name");
	}
}
