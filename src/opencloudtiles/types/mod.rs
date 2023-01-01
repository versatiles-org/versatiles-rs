mod tile_bbox;
mod tile_bbox_pyramide;
mod tile_converter_config;
mod tile_coords;
mod tile_reader_parameters;

use clap::ValueEnum;
pub use tile_bbox::TileBBox;
pub use tile_bbox_pyramide::TileBBoxPyramide;
pub use tile_converter_config::TileConverterConfig;
pub use tile_coords::{TileCoord2, TileCoord3};
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
