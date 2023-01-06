use crate::opencloudtiles::lib::DataConverter;

use super::{Precompression, TileBBoxPyramide, TileFormat};

#[derive(Debug)]
pub struct TileReaderParameters {
	tile_format: TileFormat,
	tile_precompression: Precompression,
	bbox_pyramide: TileBBoxPyramide,
	#[allow(dead_code)]
	decompressor: DataConverter,
}

impl TileReaderParameters {
	pub fn new(
		tile_format: TileFormat, tile_precompression: Precompression, bbox_pyramide: TileBBoxPyramide,
	) -> TileReaderParameters {
		let decompressor = DataConverter::new_decompressor(&tile_precompression);
		return TileReaderParameters {
			decompressor,
			tile_format,
			tile_precompression,
			bbox_pyramide,
		};
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		return &self.tile_format;
	}
	pub fn get_tile_precompression(&self) -> &Precompression {
		return &self.tile_precompression;
	}
	#[allow(dead_code)]
	pub fn get_decompressor(&self) -> &DataConverter {
		return &self.decompressor;
	}
	pub fn get_level_bbox(&self) -> &TileBBoxPyramide {
		return &self.bbox_pyramide;
	}
}
