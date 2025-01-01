#[cfg(feature = "cli")]
use super::ProbeDepth;
use super::{Blob, TileBBox, TileCompression, TileCoord3, TileStream, TilesReaderParameters};
#[cfg(feature = "cli")]
use crate::utils::PrettyPrint;
use crate::utils::TileJSON;
use anyhow::Result;
use async_trait::async_trait;
use futures::lock::Mutex;
use std::{fmt::Debug, sync::Arc};

/// Trait defining the behavior of a tile reader.
#[async_trait]
pub trait TilesReaderTrait: Debug + Send + Sync + Unpin {
	/// Get the name of the reader source, e.g., the filename.
	fn get_source_name(&self) -> &str;

	/// Get the container name, e.g., versatiles, mbtiles, etc.
	fn get_container_name(&self) -> &str;

	/// Get the reader parameters.
	fn get_parameters(&self) -> &TilesReaderParameters;

	/// Override the tile compression.
	fn override_compression(&mut self, tile_compression: TileCompression);

	/// Get the metadata, always uncompressed.
	fn get_tilejson(&self) -> &TileJSON;

	/// Get tile data for the given coordinate, always compressed and formatted.
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>>;

	/// Get a stream of tiles within the bounding box.
	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord3> = bbox.iter_coords().collect();
		TileStream::from_coord_vec_async(coords, move |coord| {
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
		})
	}

	/// probe container
	#[cfg(feature = "cli")]
	async fn probe(&mut self, level: ProbeDepth) -> Result<()> {
		use ProbeDepth::*;

		let mut print = PrettyPrint::new();

		let cat = print.get_category("meta_data").await;
		cat.add_key_value("name", self.get_source_name()).await;
		cat.add_key_value("container", self.get_container_name()).await;

		cat.add_key_value("meta", &self.get_tilejson().stringify()).await;

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
		let parameters = self.get_parameters();
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
	use super::*;
	use crate::types::{TileBBoxPyramid, TileFormat};

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
					tile_format: TileFormat::PBF,
				},
				tilejson,
			}
		}
	}

	#[async_trait]
	impl TilesReaderTrait for TestReader {
		fn get_source_name(&self) -> &str {
			"dummy"
		}

		fn get_container_name(&self) -> &str {
			"test container name"
		}

		fn get_parameters(&self) -> &TilesReaderParameters {
			&self.parameters
		}

		fn override_compression(&mut self, tile_compression: TileCompression) {
			self.parameters.tile_compression = tile_compression;
		}

		fn get_tilejson(&self) -> &TileJSON {
			&self.tilejson
		}

		async fn get_tile_data(&self, _coord: &TileCoord3) -> Result<Option<Blob>> {
			Ok(Some(Blob::from("test tile data")))
		}
	}

	#[tokio::test]
	async fn test_get_name() {
		let reader = TestReader::new_dummy();
		assert_eq!(reader.get_source_name(), "dummy");
	}

	#[tokio::test]
	async fn test_get_container_name() {
		let reader = TestReader::new_dummy();
		assert_eq!(reader.get_container_name(), "test container name");
	}

	#[tokio::test]
	async fn test_get_parameters() {
		let reader = TestReader::new_dummy();
		let parameters = reader.get_parameters();
		assert_eq!(parameters.tile_compression, TileCompression::Gzip);
		assert_eq!(parameters.tile_format, TileFormat::PBF);
		assert_eq!(parameters.bbox_pyramid.get_zoom_min().unwrap(), 0);
		assert_eq!(parameters.bbox_pyramid.get_zoom_max().unwrap(), 3);
	}

	#[tokio::test]
	async fn test_override_compression() {
		let mut reader = TestReader::new_dummy();
		assert_eq!(reader.get_parameters().tile_compression, TileCompression::Gzip);

		reader.override_compression(TileCompression::Brotli);
		assert_eq!(reader.get_parameters().tile_compression, TileCompression::Brotli);
	}

	#[tokio::test]
	async fn test_get_meta() -> Result<()> {
		let reader = TestReader::new_dummy();
		assert_eq!(reader.get_tilejson().as_string(), "test metadata");
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
	async fn test_get_bbox_tile_stream() -> Result<()> {
		let reader = TestReader::new_dummy();
		let bbox = TileBBox::new(1, 0, 0, 1, 1)?;
		let stream = reader.get_bbox_tile_stream(bbox).await;

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
}
