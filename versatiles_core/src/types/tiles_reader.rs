#[cfg(feature = "cli")]
use super::ProbeDepth;
use super::{Blob, TileBBox, TileCompression, TileCoord3, TileStream, TilesReaderParameters};
#[cfg(feature = "cli")]
use crate::utils::PrettyPrint;
use crate::{
	tilejson::TileJSON,
	types::{TraversalOrder, TraversalOrderSet},
};
use anyhow::Result;
use async_trait::async_trait;
use futures::lock::Mutex;
use std::{fmt::Debug, sync::Arc};

/// Trait defining the behavior of a tile reader.
#[async_trait]
pub trait TilesReaderTrait: Debug + Send + Sync + Unpin {
	/// Get the name of the reader source, e.g., the filename.
	fn source_name(&self) -> &str;

	/// Get the container name, e.g., versatiles, mbtiles, etc.
	fn container_name(&self) -> &str;

	/// Get the reader parameters.
	fn parameters(&self) -> &TilesReaderParameters;

	/// Override the tile compression.
	fn override_compression(&mut self, tile_compression: TileCompression);

	/// Get the metadata, always uncompressed.
	fn tilejson(&self) -> &TileJSON;

	fn traversal_orders(&self) -> TraversalOrderSet {
		TraversalOrderSet::new_all()
	}

	fn iter_bboxes(&self) -> Result<Box<dyn Iterator<Item = TileBBox> + '_ + Send>> {
		self.iter_bboxes_in_order(self.traversal_orders().get_best()?)
	}

	fn iter_bboxes_in_order(&self, order: TraversalOrder) -> Result<Box<dyn Iterator<Item = TileBBox> + '_ + Send>> {
		Ok(Box::new(self.parameters().bbox_pyramid.iter_bboxes(order)))
	}

	fn iter_bboxes_in_preferred_order(
		&self,
		orders: &[TraversalOrder],
	) -> Result<Box<dyn Iterator<Item = TileBBox> + '_ + Send>> {
		self.iter_bboxes_in_order(self.traversal_orders().get_best_of(orders)?)
	}

	/// Get tile data for the given coordinate, always compressed and formatted.
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>>;

	/// Get a stream of tiles within the bounding box.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord3> = bbox.iter_coords().collect();
		Ok(TileStream::from_coord_vec_async(coords, move |coord| {
			let mutex = mutex.clone();
			async move {
				mutex
					.lock()
					.await
					.get_tile_data(&coord)
					.await
					.map(|blob_option| blob_option.map(|blob| (coord, blob)))
					.unwrap_or(None)
			}
		}))
	}

	/// probe container
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
			self.probe_container(&print.get_category("container").await).await?;
		}

		if matches!(level, Tiles | TileContents) {
			self.probe_tiles(&print.get_category("tiles").await).await?;
		}

		if matches!(level, TileContents) {
			self
				.probe_tile_contents(&print.get_category("tile contents").await)
				.await?;
		}

		Ok(())
	}

	#[cfg(feature = "cli")]
	async fn probe_parameters(&mut self, print: &mut PrettyPrint) -> Result<()> {
		let parameters = self.parameters();
		let p = print.get_list("bbox_pyramid").await;
		for level in parameters.bbox_pyramid.iter_levels() {
			p.add_value(level).await
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

	/// deep probe container
	#[cfg(feature = "cli")]
	async fn probe_container(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this container format")
			.await;
		Ok(())
	}

	/// deep probe container tiles
	#[cfg(feature = "cli")]
	async fn probe_tiles(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tiles probing is not implemented for this container format")
			.await;
		Ok(())
	}

	/// deep probe container tile contents
	#[cfg(feature = "cli")]
	async fn probe_tile_contents(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tile contents probing is not implemented for this container format")
			.await;
		Ok(())
	}

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
	use crate::types::{TileBBoxPyramid, TileFormat, TraversalOrder, TraversalOrderSet};
	#[cfg(feature = "cli")]
	use crate::utils::PrettyPrint;

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
		fn source_name(&self) -> &str {
			"dummy"
		}

		fn container_name(&self) -> &str {
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

		async fn get_tile_data(&self, _coord: &TileCoord3) -> Result<Option<Blob>> {
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
		assert_eq!(parameters.bbox_pyramid.get_zoom_min().unwrap(), 0);
		assert_eq!(parameters.bbox_pyramid.get_zoom_max().unwrap(), 3);
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
	async fn test_get_tile_data() -> Result<()> {
		let reader = TestReader::new_dummy();
		let coord = TileCoord3::new(0, 0, 0)?;
		let tile_data = reader.get_tile_data(&coord).await?;
		assert_eq!(tile_data, Some(Blob::from("test tile data")));
		Ok(())
	}

	#[tokio::test]
	async fn test_get_tile_stream() -> Result<()> {
		let reader = TestReader::new_dummy();
		let bbox = TileBBox::new(1, 0, 0, 1, 1)?;
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
	async fn test_traversal_orders_default() {
		let reader = TestReader::new_dummy();
		let default_orders = reader.traversal_orders();
		assert_eq!(default_orders, TraversalOrderSet::new_all());
	}

	#[tokio::test]
	async fn test_iter_bboxes_non_empty() {
		let reader = TestReader::new_dummy();
		let mut bboxes = reader.iter_bboxes().unwrap();
		assert_eq!(bboxes.next().unwrap().as_string(), "0:[0,0,0,0]");
	}

	#[tokio::test]
	async fn test_iter_bboxes_in_preferred_order() {
		let reader = TestReader::new_dummy();
		// Use a single preferred order
		let order = TraversalOrder::BottomUp;
		let mut bboxes = reader.iter_bboxes_in_preferred_order(&[order]).unwrap();
		assert_eq!(bboxes.next().unwrap().as_string(), "3:[0,0,7,7]");
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
