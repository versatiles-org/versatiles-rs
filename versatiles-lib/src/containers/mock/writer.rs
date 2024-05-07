use crate::{
	containers::{TilesReaderBox, TilesWriterBox, TilesWriterTrait},
	shared::{Compression, TileBBoxPyramid, TileFormat, TilesWriterConfig},
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use std::path::Path;

#[allow(dead_code)]
#[derive(Debug)]
pub enum MockTilesWriterProfile {
	PNG,
	Whatever,
}

pub struct MockTilesWriter {
	config: TilesWriterConfig,
}

impl MockTilesWriter {
	pub fn new_mock(profile: MockTilesWriterProfile, max_zoom_level: u8) -> TilesWriterBox {
		let mut bbox_pyramid = TileBBoxPyramid::new_full();
		bbox_pyramid.set_zoom_max(max_zoom_level);

		let config = match profile {
			MockTilesWriterProfile::PNG => {
				TilesWriterConfig::new(Some(TileFormat::PNG), Some(Compression::None), bbox_pyramid, false)
			}
			MockTilesWriterProfile::Whatever => TilesWriterConfig::new(None, None, bbox_pyramid, false),
		};
		Box::new(MockTilesWriter { config })
	}
}

#[async_trait]
impl TilesWriterTrait for MockTilesWriter {
	async fn open_file(_path: &Path, config: TilesWriterConfig) -> Result<TilesWriterBox>
	where
		Self: Sized,
	{
		Ok(Box::new(Self { config }))
	}
	async fn convert_from(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
		let _temp = reader.get_container_name();
		let _temp = reader.get_name();
		let _temp = reader.get_meta().await?;

		self.config.finalize_with_parameters(reader.get_parameters());
		let bbox_pyramid = self.config.get_bbox_pyramid();

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(*bbox).await;
			while let Some((_coord, _blob)) = stream.next().await {}
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::{MockTilesWriter, MockTilesWriterProfile};
	use crate::{
		containers::{
			mock::{reader::MockTilesReaderProfile, MockTilesReader},
			TilesWriterTrait,
		},
		shared::TilesWriterConfig,
	};
	use std::path::Path;

	#[tokio::test]
	async fn convert_from() {
		let mut converter = MockTilesWriter::new_mock(MockTilesWriterProfile::PNG, 8);
		let mut reader = MockTilesReader::new_mock(MockTilesReaderProfile::PNG, 8);
		converter.convert_from(&mut reader).await.unwrap();
	}

	#[tokio::test]
	async fn dummy() {
		MockTilesWriter::open_file(&Path::new("hi"), TilesWriterConfig::new_full())
			.await
			.unwrap();
	}
}
