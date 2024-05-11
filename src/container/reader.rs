#[cfg(feature = "full")]
use crate::{container::ProbeDepth, helper::pretty_print::PrettyPrint};
use crate::{
	container::TilesStream,
	types::{Blob, TileBBox, TileBBoxPyramid, TileCompression, TileCoord3, TileFormat},
};
use anyhow::Result;
use async_trait::async_trait;
use futures::{stream, StreamExt};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;

pub type TilesReaderBox = Box<dyn TilesReaderTrait>;

#[derive(Debug, PartialEq, Clone)]
pub struct TilesReaderParameters {
	pub bbox_pyramid: TileBBoxPyramid,
	pub tile_compression: TileCompression,
	pub tile_format: TileFormat,
}
impl TilesReaderParameters {
	pub fn new(
		tile_format: TileFormat, tile_compression: TileCompression, bbox_pyramid: TileBBoxPyramid,
	) -> TilesReaderParameters {
		TilesReaderParameters {
			tile_format,
			tile_compression,
			bbox_pyramid,
		}
	}
}

#[async_trait]
pub trait TilesReaderTrait: Debug + Send + Sync + Unpin {
	/// some kine of name for this reader source, e.g. the filename
	fn get_name(&self) -> &str;

	/// container name, e.g. versatiles, mbtiles, ...
	fn get_container_name(&self) -> &str;

	fn get_parameters(&self) -> &TilesReaderParameters;

	fn override_compression(&mut self, tile_compression: TileCompression);

	/// get meta data, always uncompressed
	fn get_meta(&self) -> Result<Option<Blob>>;

	/// always compressed with tile_compression and formatted with get_tile_format
	/// returns the tile in the coordinate system of the source
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>>;

	/// always compressed with get_tile_compression and formatted with get_tile_format
	/// returns the tiles in the coordinate system of the source
	async fn get_bbox_tile_stream(&mut self, bbox: &TileBBox) -> TilesStream {
		let mutex = Arc::new(Mutex::new(self));
		let coords: Vec<TileCoord3> = bbox.iter_coords().collect();
		stream::iter(coords)
			.filter_map(move |coord| {
				let mutex = mutex.clone();
				async move {
					mutex
						.lock()
						.await
						.get_tile_data(&coord)
						.map(|blob_option| blob_option.map(|blob| (coord, blob)))
						.unwrap_or(None)
				}
			})
			.boxed()
	}

	#[cfg(feature = "full")]
	/// probe container
	async fn probe(&mut self, level: ProbeDepth) -> Result<()> {
		use ProbeDepth::*;

		let mut print = PrettyPrint::new();

		let cat = print.get_category("meta_data").await;
		cat.add_key_value("name", self.get_name()).await;
		cat.add_key_value("container", self.get_container_name()).await;

		let meta_option = self.get_meta()?;
		if let Some(meta) = meta_option {
			cat.add_key_value("meta", meta.as_str()).await;
		} else {
			cat.add_key_value("meta", &meta_option).await;
		}

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

	#[cfg(feature = "full")]
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

	#[cfg(feature = "full")]
	/// deep probe container
	async fn probe_container(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep container probing is not implemented for this container format")
			.await;
		Ok(())
	}

	#[cfg(feature = "full")]
	/// deep probe container tiles
	async fn probe_tiles(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tiles probing is not implemented for this container format")
			.await;
		Ok(())
	}

	#[cfg(feature = "full")]
	/// deep probe container tile contents
	async fn probe_tile_contents(&mut self, print: &PrettyPrint) -> Result<()> {
		print
			.add_warning("deep tile contents probing is not implemented for this container format")
			.await;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[derive(Debug)]
	struct TestReader {
		parameters: TilesReaderParameters,
	}

	impl TestReader {
		fn new_dummy() -> TilesReaderBox {
			Box::new(TestReader {
				parameters: TilesReaderParameters {
					bbox_pyramid: TileBBoxPyramid::new_full(3),
					tile_compression: TileCompression::Gzip,
					tile_format: TileFormat::PBF,
				},
			})
		}
	}

	#[async_trait]
	impl TilesReaderTrait for TestReader {
		fn get_name(&self) -> &str {
			"dummy"
		}
		fn get_parameters(&self) -> &TilesReaderParameters {
			&self.parameters
		}
		fn override_compression(&mut self, tile_compression: TileCompression) {
			self.parameters.tile_compression = tile_compression;
		}
		fn get_meta(&self) -> Result<Option<Blob>> {
			Ok(Some(Blob::from("test metadata")))
		}
		fn get_container_name(&self) -> &str {
			"test container name"
		}
		fn get_tile_data(&mut self, _coord: &TileCoord3) -> Result<Option<Blob>> {
			Ok(Some(Blob::from("test tile data")))
		}
	}

	#[tokio::test]
	#[cfg(feature = "full")]
	async fn reader() -> Result<()> {
		use crate::container::mock::MockTilesWriter;

		let mut reader = TestReader::new_dummy();

		// Test getting name
		assert_eq!(reader.get_name(), "dummy");

		// Test getting tile compression and format
		let parameters = reader.get_parameters();
		assert_eq!(parameters.tile_compression, TileCompression::Gzip);
		assert_eq!(parameters.tile_format, TileFormat::PBF);

		// Test getting container name
		assert_eq!(reader.get_container_name(), "test container name");

		// Test getting metadata
		assert_eq!(reader.get_meta()?.unwrap().to_string(), "test metadata");

		// Test getting tile data
		let coord = TileCoord3::new(0, 0, 0)?;
		assert_eq!(reader.get_tile_data(&coord)?.unwrap().to_string(), "test tile data");

		let mut writer = MockTilesWriter::new_mock();
		writer.write_from_reader(&mut reader).await?;

		Ok(())
	}

	#[tokio::test]
	async fn get_bbox_tile_iter() -> Result<()> {
		let mut reader = TestReader::new_dummy();
		let bbox = TileBBox::new(4, 0, 0, 10, 10)?; // Or replace it with actual bbox
		let mut stream = reader.get_bbox_tile_stream(&bbox).await;

		while let Some((_coord, _blob)) = stream.next().await {}

		Ok(())
	}
}
