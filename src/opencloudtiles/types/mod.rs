mod tile_bbox;
mod tile_bbox_pyramide;
mod tile_converter_config;
mod tile_coords;
mod tile_reader_parameters;

use clap::ValueEnum;
use enumset::EnumSetType;
pub use tile_bbox::TileBBox;
pub use tile_bbox_pyramide::TileBBoxPyramide;
pub use tile_converter_config::TileConverterConfig;
pub use tile_coords::{TileCoord2, TileCoord3};
pub use tile_reader_parameters::TileReaderParameters;

#[derive(Clone, Debug, PartialEq, ValueEnum)]
pub enum TileFormat {
	PBF,
	PNG,
	JPG,
	WEBP,
}

#[derive(Debug, EnumSetType, ValueEnum)]
pub enum Compression {
	Uncompressed,
	Gzip,
	Brotli,
}

pub type TileData = Vec<u8>;
pub type MetaData = Vec<u8>;
