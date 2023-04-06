use crate::{TileConverterBox, TileConverterTrait, TileReaderBox};
use async_trait::async_trait;
use std::path::Path;
use versatiles_shared::{Precompression, TileBBoxPyramide, TileConverterConfig, TileFormat};

pub enum DummyConverterProfile {
	Png,
}

pub struct TileConverter {
	config: TileConverterConfig,
}

impl TileConverter {
	pub fn new_dummy(profile: DummyConverterProfile, max_zoom_level: u8) -> TileConverterBox {
		let mut bbox_pyramide = TileBBoxPyramide::new_full();
		bbox_pyramide.set_zoom_max(max_zoom_level);

		let config = match profile {
			DummyConverterProfile::Png => TileConverterConfig::new(
				Some(TileFormat::PNG),
				Some(Precompression::Uncompressed),
				bbox_pyramide,
				false,
			),
		};
		Box::new(TileConverter { config })
	}
}

#[async_trait]
impl TileConverterTrait for TileConverter {
	fn new(_filename: &Path, config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized,
	{
		Box::new(Self { config })
	}
	async fn convert_from(&mut self, reader: &mut TileReaderBox) {
		reader.get_container_name();
		reader.get_name();
		reader.get_meta().await;
		self.config.finalize_with_parameters(reader.get_parameters());
		let bbox_pyramide = self.config.get_bbox_pyramide();

		for (level, bbox) in bbox_pyramide.iter_levels() {
			for row_bbox in bbox.iter_bbox_row_slices(1024) {
				let _tile_vec = reader.get_bbox_tile_vec(level, &row_bbox).await;
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{DummyConverterProfile, TileConverter};
	use crate::dummy::{reader::DummyReaderProfile, TileReader};
	use futures::executor::block_on;

	#[test]
	fn test() {
		let mut converter = TileConverter::new_dummy(DummyConverterProfile::Png, 8);
		let mut reader = TileReader::new_dummy(DummyReaderProfile::PngEmpty, 8);
		block_on(converter.convert_from(&mut reader));
	}
}
