use crate::{
	containers::{TilesConverterBox, TilesConverterTrait, TilesReaderBox},
	shared::{Compression, TileBBoxPyramid, TileFormat, TilesConverterConfig},
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use std::path::Path;

#[allow(dead_code)]
#[derive(Debug)]
pub enum MockTilesConverterProfile {
	PNG,
	Whatever,
}

pub struct MockTilesConverter {
	config: TilesConverterConfig,
}

impl MockTilesConverter {
	pub fn new_mock(profile: MockTilesConverterProfile, max_zoom_level: u8) -> TilesConverterBox {
		let mut bbox_pyramid = TileBBoxPyramid::new_full();
		bbox_pyramid.set_zoom_max(max_zoom_level);

		let config = match profile {
			MockTilesConverterProfile::PNG => {
				TilesConverterConfig::new(Some(TileFormat::PNG), Some(Compression::None), bbox_pyramid, false)
			}
			MockTilesConverterProfile::Whatever => TilesConverterConfig::new(None, None, bbox_pyramid, false),
		};
		Box::new(MockTilesConverter { config })
	}
}

#[async_trait]
impl TilesConverterTrait for MockTilesConverter {
	async fn open_file(_path: &Path, config: TilesConverterConfig) -> Result<TilesConverterBox>
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
	use super::{MockTilesConverter, MockTilesConverterProfile};
	use crate::{
		containers::{
			mock::{reader::MockTilesReaderProfile, MockTilesReader},
			TilesConverterTrait,
		},
		shared::TilesConverterConfig,
	};
	use std::path::Path;

	#[tokio::test]
	async fn convert_from() {
		let mut converter = MockTilesConverter::new_mock(MockTilesConverterProfile::PNG, 8);
		let mut reader = MockTilesReader::new_mock(MockTilesReaderProfile::PNG, 8);
		converter.convert_from(&mut reader).await.unwrap();
	}

	#[tokio::test]
	async fn dummy() {
		MockTilesConverter::open_file(&Path::new("hi"), TilesConverterConfig::new_full())
			.await
			.unwrap();
	}
}
