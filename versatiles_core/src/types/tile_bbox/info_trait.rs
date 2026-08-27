use super::super::tile_cover::info_trait::TileCoverInfo;
use crate::TileBBox;

impl TileCoverInfo for TileBBox {
	fn level(&self) -> u8 {
		TileBBox::level(self)
	}

	fn type_name(&self) -> &'static str {
		"TileBBox"
	}
}
