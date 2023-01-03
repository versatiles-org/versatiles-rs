use super::{TileBBoxPyramide, TileFormat};

#[derive(Debug)]
pub struct TileReaderParameters {
	tile_format: TileFormat,
	bbox_pyramide: TileBBoxPyramide,
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
