#![allow(unused_variables)]

mod converter;
mod reader;

use clap::ValueEnum;
pub use converter::{TileConverter, TileConverterConfig};
pub use reader::{TileReader, TileReaderParameters, TileReaderWrapper};

#[derive(PartialEq, Clone, Debug, ValueEnum)]
pub enum TileFormat {
	PBF,
	PBFGzip,
	PBFBrotli,
	PNG,
	JPG,
	WEBP,
}

pub type Tile = Vec<u8>;
