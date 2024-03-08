use crate::{
	containers::{TileConverterBox, TileConverterTrait, TileReaderBox},
	shared::{Compression, TileBBoxPyramid, TileConverterConfig, TileFormat},
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;

#[allow(dead_code)]
#[derive(Debug)]
pub enum ConverterProfile {
	PNG,
	Whatever,
}

pub struct TileConverter {
	config: TileConverterConfig,
}

impl TileConverter {
	pub fn new_mock(profile: ConverterProfile, max_zoom_level: u8) -> TileConverterBox {
		let mut bbox_pyramid = TileBBoxPyramid::new_full();
		bbox_pyramid.set_zoom_max(max_zoom_level);

		let config = match profile {
			ConverterProfile::PNG => {
				TileConverterConfig::new(Some(TileFormat::PNG), Some(Compression::None), bbox_pyramid, false)
			}
			ConverterProfile::Whatever => TileConverterConfig::new(None, None, bbox_pyramid, false),
		};
		Box::new(TileConverter { config })
	}
}

#[async_trait]
impl TileConverterTrait for TileConverter {
	async fn new(_filename: &str, config: TileConverterConfig) -> Result<TileConverterBox>
	where
		Self: Sized,
	{
		Ok(Box::new(Self { config }))
	}
	async fn convert_from(&mut self, reader: &mut TileReaderBox) -> Result<()> {
		let _temp = reader.get_container_name()?;
		let _temp = reader.get_name()?;
		let _temp = reader.get_meta().await?;

		self.config.finalize_with_parameters(reader.get_parameters()?);
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
	use super::{ConverterProfile, TileConverter};
	use crate::{
		containers::{
			mock::{reader::ReaderProfile, TileReader},
			TileConverterTrait,
		},
		shared::TileConverterConfig,
	};

	#[tokio::test]
	async fn convert_from() {
		let mut converter = TileConverter::new_mock(ConverterProfile::PNG, 8);
		let mut reader = TileReader::new_mock(ReaderProfile::PNG, 8);
		converter.convert_from(&mut reader).await.unwrap();
	}

	#[tokio::test]
	async fn dummy() {
		TileConverter::new("hi", TileConverterConfig::new_full()).await.unwrap();
	}
}
