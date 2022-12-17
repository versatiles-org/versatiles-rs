#![allow(unused_variables)]

use std::path::PathBuf;

#[derive(PartialEq, Clone)]
pub enum TileFormat {
	PBF,
	PNG,
	JPG,
	WEBP,
}
#[derive(PartialEq, Clone)]
pub enum TileCompression {
	None,
	Gzip,
	Brotli,
}

pub trait Reader {
	#[allow(dead_code)]
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
		panic!("not implemented: set_maximum_zoom");
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
	fn convert_from(filename: &PathBuf, container: Box<dyn Reader>) -> std::io::Result<()> {
		panic!("not implemented: convert_from");
	}
}
