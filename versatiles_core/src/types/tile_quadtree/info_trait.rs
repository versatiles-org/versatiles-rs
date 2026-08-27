use super::super::tile_cover::info_trait::TileCoverInfo;
use crate::TileQuadtree;

impl TileCoverInfo for TileQuadtree {
	fn level(&self) -> u8 {
		TileQuadtree::level(self)
	}

	fn type_name(&self) -> &'static str {
		"TileQuadtree"
	}
}
