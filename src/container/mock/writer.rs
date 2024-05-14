use crate::{
	container::{TilesReader, TilesWriter},
	helper::DataWriterTrait,
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;

pub struct MockTilesWriter {}

impl MockTilesWriter {
	pub async fn write(reader: &mut dyn TilesReader) -> Result<()> {
		let _temp = reader.get_container_name();
		let _temp = reader.get_name();
		let _temp = reader.get_meta()?;

		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox).await;
			while let Some((_coord, _blob)) = stream.next().await {}
		}

		Ok(())
	}
}

#[async_trait]
impl TilesWriter for MockTilesWriter {
	async fn write_to_writer(reader: &mut dyn TilesReader, _writer: &mut dyn DataWriterTrait) -> Result<()> {
		MockTilesWriter::write(reader).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::container::mock::{MockTilesReader, MockTilesReaderProfile};

	#[tokio::test]
	async fn convert_png() -> Result<()> {
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PNG)?;
		MockTilesWriter::write(&mut reader).await?;
		Ok(())
	}

	#[tokio::test]
	async fn convert_pbf() -> Result<()> {
		let mut reader = MockTilesReader::new_mock_profile(MockTilesReaderProfile::PBF)?;
		MockTilesWriter::write(&mut reader).await?;
		Ok(())
	}
}
