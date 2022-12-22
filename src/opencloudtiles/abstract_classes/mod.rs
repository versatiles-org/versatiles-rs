#![allow(unused_variables)]

mod converter;
mod reader;

pub use converter::{TileConverter, TileConverterConfig};
pub use reader::{TileReader, TileReaderWrapper};

use clap::ValueEnum;

#[derive(PartialEq, Clone, Debug)]
pub enum TileFormat {
	PBF,
	PNG,
	JPG,
	WEBP,
}

#[derive(PartialEq, Clone, Debug, ValueEnum)]
pub enum TileCompression {
	/// uncompressed
	None,
	/// use gzip
	Gzip,
	/// use brotli
	Brotli,
}
