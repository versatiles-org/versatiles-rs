//! Utilities for reading and probing tile data from various container formats.
//!
//! This module defines the `TilesReaderTrait` with methods for traversing,
//! retrieving, and probing tile metadata, parameters, container info, and contents.

#[cfg(feature = "cli")]
use super::ProbeDepth;
#[cfg(feature = "cli")]
use crate::utils::PrettyPrint;
use crate::{
	Blob, TileBBox, TileCompression, TileCoord, TileJSON, TileStream, TilesReaderParameters, Traversal,
	TraversalTranslationStep, cache::CacheMap, config::Config, progress::get_progress_bar, translate_traversals,
};
use anyhow::Result;
use async_trait::async_trait;
use futures::{StreamExt, future::BoxFuture, stream};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;

/// Trait defining behavior for reading tiles from a container.
///
/// Implementors provide tile metadata, traversal orders, bounding boxes,
/// data retrieval, and CLI probing methods.
#[async_trait]
pub trait TilesReaderTrait: Debug + Send + Sync + Unpin {
	/// Return the source identifier (e.g., filename or URI).
	fn source_name(&self) -> &str;

	/// Return the container type name (e.g., "mbtiles", "versatiles").
	fn container_name(&self) -> &str;

	/// Access the reader parameters, including bounding box pyramid and formats.
	fn parameters(&self) -> &TilesReaderParameters;

	/// Override the default tile compression for subsequent reads.
	fn override_compression(&mut self, tile_compression: TileCompression);

	/// Retrieve the `TileJSON` metadata for this tile set.
	fn tilejson(&self) -> &TileJSON;

	/// Return the supported traversal order.
	fn traversal(&self) -> &Traversal {
		&Traversal::ANY
	}

	/// Traverse all tiles in the given traversal order, invoking `callback` for each bbox and its tile stream.
	/// Runs sequentially; awaits each callback before moving to the next.
	async fn traverse_all_tiles<'a>(
		&'a self,
		traversal_write: &Traversal,
		mut callback: Box<dyn 'a + Send + FnMut(TileBBox, TileStream<'a>) -> BoxFuture<'a, Result<()>>>,
		config: Arc<Config>,
	) -> Result<()> {
		let traversal_steps = translate_traversals(&self.parameters().bbox_pyramid, self.traversal(), traversal_write)?;

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

		let cache = Arc::new(Mutex::new(CacheMap::<usize, (TileCoord, Blob)>::new(config)));
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

	/// Asynchronously fetch the raw tile data for the given tile coordinate.
	async fn get_tile_blob(&self, coord: &TileCoord) -> Result<Option<Blob>>;

	/// Asynchronously stream all tiles within the given bounding box.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord> = bbox.iter_coords().collect();
		Ok(TileStream::from_coord_vec_async(coords, move |coord| {
			let mutex = mutex.clone();
			async move {
				mutex
					.lock()
					.await
					.get_tile_blob(&coord)
					.await
					.map(|blob_option| blob_option.map(|blob| (coord, blob)))
					.unwrap_or(None)
			}
		}))
	}

	/// Asynchronously stream the sizes of all tiles within the given bounding box.
	async fn get_tile_size_stream(&self, bbox: TileBBox) -> Result<TileStream<u64>> {
		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord> = bbox.iter_coords().collect();
		Ok(TileStream::from_coord_vec_async(coords, move |coord| {
			let mutex = mutex.clone();
			async move {
				mutex
					.lock()
					.await
					.get_tile_blob(&coord)
					.await
					.map(|blob_option| blob_option.map(|blob| (coord, blob.len())))
					.unwrap_or(None)
			}
		}))
	}

	/// Perform a hierarchical CLI probe of metadata, parameters, container, tiles, and contents.
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

	/// Probe and print reader parameters (bbox levels, formats, compression).
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

	/// Probe and print container-specific metadata or warnings.
	#[cfg(feature = "cli")]
	async fn probe_container(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this container format")
			.await;
		Ok(())
	}

	/// Probe and print tile-specific metadata or warnings.
	#[cfg(feature = "cli")]
	async fn probe_tiles(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tiles probing is not implemented for this container format")
			.await;
		Ok(())
	}

	/// Probe and print contents of sample tiles or warnings if unimplemented.
	#[cfg(feature = "cli")]
	async fn probe_tile_contents(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tile contents probing is not implemented for this container format")
			.await;
		Ok(())
	}

	/// Convert the reader into a boxed trait object for dynamic dispatch.
	fn boxed(self) -> Box<dyn TilesReaderTrait>
	where
		Self: Sized + 'static,
	{
		Box::new(self)
	}
}

#[cfg(test)]
mod tests {
	#[cfg(feature = "cli")]
	use super::ProbeDepth;
	use super::*;
	#[cfg(feature = "cli")]
	use crate::utils::PrettyPrint;
	use crate::{TileBBoxPyramid, TileFormat};

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

		async fn get_tile_blob(&self, _coord: &TileCoord) -> Result<Option<Blob>> {
			Ok(Some(Blob::from("test tile data")))
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
	async fn test_get_tile_blob() -> Result<()> {
		let reader = TestReader::new_dummy();
		let coord = TileCoord::new(0, 0, 0)?;
		let tile_data = reader.get_tile_blob(&coord).await?;
		assert_eq!(tile_data, Some(Blob::from("test tile data")));
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
			use crate::utils::PrettyPrint;

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
