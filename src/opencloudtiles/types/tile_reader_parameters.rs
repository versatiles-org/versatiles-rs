use super::{Compression, TileBBoxPyramide, TileFormat};

#[derive(Debug)]
pub struct TileReaderParameters {
	tile_format: TileFormat,
	tile_precompression: Compression,
	bbox_pyramide: TileBBoxPyramide,
}

impl TileReaderParameters {
	pub fn new(
		tile_format: TileFormat, tile_precompression: Compression, bbox_pyramide: TileBBoxPyramide,
	) -> TileReaderParameters {
		return TileReaderParameters {
			tile_format,
			tile_precompression,
			bbox_pyramide,
		};
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		return &self.tile_format;
	}
	pub fn get_tile_precompression(&self) -> &Compression {
		return &self.tile_precompression;
	}
	pub fn get_level_bbox(&self) -> &TileBBoxPyramide {
		return &self.bbox_pyramide;
	}
}
