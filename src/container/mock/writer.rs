use crate::container::{TilesReaderBox, TilesWriterBox, TilesWriterTrait};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;

pub struct MockTilesWriter {}

impl MockTilesWriter {
	pub fn new_mock() -> TilesWriterBox {
		Box::new(MockTilesWriter {})
	}
}

#[async_trait]
impl TilesWriterTrait for MockTilesWriter {
	async fn write_from_reader(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
		let _temp = reader.get_container_name();
		let _temp = reader.get_name();
		let _temp = reader.get_meta().await?;

		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox).await;
			while let Some((_coord, _blob)) = stream.next().await {}
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::container::{MockTilesReader, MockTilesReaderProfile};

	#[tokio::test]
	async fn convert_png() {
		let mut writer = MockTilesWriter::new_mock();
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PNG);
		writer.write_from_reader(&mut reader).await.unwrap();
	}

	#[tokio::test]
	async fn convert_pbf() {
		let mut writer = MockTilesWriter::new_mock();
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PBF);
		writer.write_from_reader(&mut reader).await.unwrap();
	}
}
