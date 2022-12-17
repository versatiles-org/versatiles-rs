#![allow(unused_variables)]

use std::path::PathBuf;

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

pub trait Reader {
	fn load(filename: &PathBuf) -> std::io::Result<Box<dyn Reader>>
	where
		Self: Sized,
	{
		panic!("not implemented: load");
	}
	fn get_tile_format(&self) -> TileFormat {
		panic!("not implemented: get_tile_format");
	}
	fn get_tile_compression(&self) -> TileCompression {
		panic!("not implemented: get_tile_compression");
	}
	fn get_meta(&self) -> &[u8] {
		panic!("not implemented: get_meta");
	}
	fn get_minimum_zoom(&self) -> u64 {
		panic!("not implemented: get_minimum_zoom")
	}
	fn get_maximum_zoom(&self) -> u64 {
		panic!("not implemented: get_maximum_zoom");
	}
	fn set_minimum_zoom(&mut self, level: u64) {
		panic!("not implemented: set_minimum_zoom")
	}
	fn set_maximum_zoom(&mut self, level: u64) {
		panic!("not implemented: set_maximum_zoom")
	}
	fn get_minimum_col(&self, level: u64) -> u64 {
		panic!("not implemented: get_minimum_col")
	}
	fn get_maximum_col(&self, level: u64) -> u64 {
		panic!("not implemented: get_maximum_col");
	}
	fn get_minimum_row(&self, level: u64) -> u64 {
		panic!("not implemented: get_minimum_row")
	}
	fn get_maximum_row(&self, level: u64) -> u64 {
		panic!("not implemented: get_maximum_row");
	}
	fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Result<Vec<u8>, &str> {
		panic!("not implemented: get_tile_uncompressed");
	}
	fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Result<Vec<u8>, &str> {
		panic!("not implemented: get_tile_raw");
	}
}

pub trait Converter {
	fn new(filename: &PathBuf) -> std::io::Result<Box<dyn Converter>>
	where
		Self: Sized,
	{
		panic!("not implemented: new");
	}
	fn convert_from(&mut self, container: Box<dyn Reader>) -> std::io::Result<()> {
		panic!("not implemented: convert_from");
	}
	fn set_precompression(&mut self, compression: &TileCompression) {
		panic!("not implemented: set_precompression");
	}
}
