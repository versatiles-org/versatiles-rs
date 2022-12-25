use super::{tile_bbox_pyramide::TileBBoxPyramide, TileFormat};

pub struct TileReaderParameters {
	bbox_pyramide: TileBBoxPyramide,
	tile_format: TileFormat,
}

impl TileReaderParameters {
	pub fn new(tile_format: TileFormat, bbox_pyramide: TileBBoxPyramide) -> TileReaderParameters {
		return TileReaderParameters {
			tile_format,
			bbox_pyramide,
		};
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		return &self.tile_format;
	}
	pub fn get_level_bbox(&self) -> &TileBBoxPyramide {
		return &self.bbox_pyramide;
	}
}
