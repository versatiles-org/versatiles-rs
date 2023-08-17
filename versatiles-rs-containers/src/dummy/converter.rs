use crate::{TileConverterBox, TileConverterTrait, TileReaderBox};
use async_trait::async_trait;
use shared::{Compression, Result, TileBBoxPyramid, TileConverterConfig, TileFormat};

#[derive(Debug)]
pub enum ConverterProfile {
	Png,
	Whatever,
}

pub struct TileConverter {
	config: TileConverterConfig,
}

impl TileConverter {
	pub fn new_dummy(profile: ConverterProfile, max_zoom_level: u8) -> TileConverterBox {
		let mut bbox_pyramid = TileBBoxPyramid::new_full();
		bbox_pyramid.set_zoom_max(max_zoom_level);

		let config = match profile {
			ConverterProfile::Png => {
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
			let stream = reader.get_bbox_tile_vec(bbox).await?;
			let _count = stream.len();
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		dummy::{ReaderProfile, TileReader},
		TileConverterTrait,
	};
	use shared::TileConverterConfig;

	#[tokio::test]
	async fn convert_from() {
		let mut converter = TileConverter::new_dummy(ConverterProfile::Png, 8);
		let mut reader = TileReader::new_dummy(ReaderProfile::PngFast, 8);
		converter.convert_from(&mut reader).await.unwrap();
	}

	#[tokio::test]
	async fn dummy() {
		TileConverter::new("hi", TileConverterConfig::new_full()).await.unwrap();
	}
}
