use crate::{
	containers::{TilesReaderBox, TilesWriterBox, TilesWriterParameters, TilesWriterTrait},
	shared::{Compression, TileBBoxPyramid, TileFormat},
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;

#[allow(dead_code)]
#[derive(Debug)]
pub enum MockTilesWriterProfile {
	PNG,
	PBF,
}

pub struct MockTilesWriter {
	parameters: TilesWriterParameters,
}

impl MockTilesWriter {
	pub fn new_mock(profile: MockTilesWriterProfile, max_zoom_level: u8) -> TilesWriterBox {
		let mut bbox_pyramid = TileBBoxPyramid::new_full();
		bbox_pyramid.set_zoom_max(max_zoom_level);

		let config = match profile {
			MockTilesWriterProfile::PNG => TilesWriterParameters::new(TileFormat::PNG, Compression::None),
			MockTilesWriterProfile::PBF => TilesWriterParameters::new(TileFormat::PBF, Compression::Gzip),
		};
		Box::new(MockTilesWriter { parameters: config })
	}
}

#[async_trait]
impl TilesWriterTrait for MockTilesWriter {
	async fn write_tiles(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
		let _temp = reader.get_container_name();
		let _temp = reader.get_name();
		let _temp = reader.get_meta().await?;

		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(*bbox).await;
			while let Some((_coord, _blob)) = stream.next().await {}
		}

		Ok(())
	}
	fn get_parameters(&self) -> &TilesWriterParameters {
		&self.parameters
	}
}

#[cfg(test)]
mod tests {
	use super::{MockTilesWriter, MockTilesWriterProfile};
	use crate::containers::mock::{reader::MockTilesReaderProfile, MockTilesReader};

	#[tokio::test]
	async fn convert_png() {
		let mut writer = MockTilesWriter::new_mock(MockTilesWriterProfile::PNG, 8);
		let mut reader = MockTilesReader::new_mock(MockTilesReaderProfile::PNG, 8);
		writer.write_from_reader(&mut reader).await.unwrap();
	}

	#[tokio::test]
	async fn convert_pbf() {
		let mut writer = MockTilesWriter::new_mock(MockTilesWriterProfile::PBF, 8);
		let mut reader = MockTilesReader::new_mock(MockTilesReaderProfile::PBF, 8);
		writer.write_from_reader(&mut reader).await.unwrap();
	}
}
