use super::super::tile_cover::info_trait::TileCoverInfo;
use crate::TileBBox;

impl TileCoverInfo for TileBBox {
	fn get_level(&self) -> u8 {
		self.level()
	}

	fn get_type_name(&self) -> &'static str {
		"TileBBox"
	}
}
