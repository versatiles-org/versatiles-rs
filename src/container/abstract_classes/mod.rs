#![allow(unused_variables)]

mod converter;
mod reader;

pub use converter::Converter;
pub use reader::{Reader, ReaderWrapper};

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
