mod tile_bbox;
mod tile_bbox_pyramide;
mod tile_converter_config;
mod tile_reader_parameters;

use crate::opencloudtiles::{compress::*, containers::abstract_container};
use clap::ValueEnum;
pub use tile_bbox::TileBBox;
pub use tile_bbox_pyramide::TileBBoxPyramide;
pub use tile_converter_config::TileConverterConfig;
pub use tile_reader_parameters::TileReaderParameters;

#[derive(PartialEq, Clone, Debug, ValueEnum)]
pub enum TileFormat {
	PBF,
	PBFGzip,
	PBFBrotli,
	PNG,
	JPG,
	WEBP,
}

pub type TileData = Vec<u8>;

pub struct TileReaderWrapper<'a> {
	reader: &'a Box<dyn abstract_container::TileReader>,
}

impl TileReaderWrapper<'_> {
	pub fn new(reader: &Box<dyn abstract_container::TileReader>) -> TileReaderWrapper {
		return TileReaderWrapper { reader };
	}
	pub fn get_tile_data(&self, level: u64, col: u64, row: u64) -> Option<TileData> {
		return self.reader.get_tile_data(level, col, row);
	}
}

unsafe impl Send for TileReaderWrapper<'_> {}
unsafe impl Sync for TileReaderWrapper<'_> {}
