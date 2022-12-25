use super::{tile_bbox_pyramide::TileBBoxPyramide, TileFormat};

pub struct TileReaderParameters {
	zoom_min: u64,
	zoom_max: u64,
	level_bbox: TileBBoxPyramide,
	tile_format: TileFormat,
}

impl TileReaderParameters {
	pub fn new(
		zoom_min: u64,
		zoom_max: u64,
		tile_format: TileFormat,
		level_bbox: TileBBoxPyramide,
	) -> TileReaderParameters {
		return TileReaderParameters {
			zoom_min,
			zoom_max,
			tile_format,
			level_bbox,
		};
	}
	pub fn get_zoom_min(&self) -> u64 {
		return self.zoom_min;
	}
	pub fn get_zoom_max(&self) -> u64 {
		return self.zoom_max;
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		return &self.tile_format;
	}
	pub fn get_level_bbox(&self) -> &TileBBoxPyramide {
		return &self.level_bbox;
	}
}
