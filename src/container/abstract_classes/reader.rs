#![allow(unused_variables)]

use std::path::PathBuf;

use super::{TileCompression, TileFormat};

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
	fn get_level_bbox(&self, level: u64) -> (u64, u64, u64, u64) {
		panic!("not implemented: get_bbox")
	}
	fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		panic!("not implemented: get_tile_uncompressed");
	}
	fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		panic!("not implemented: get_tile_raw");
	}
}

pub struct ReaderWrapper<'a> {
	reader: &'a Box<dyn Reader>,
}

impl ReaderWrapper<'_> {
	pub fn new(reader: &Box<dyn Reader>) -> ReaderWrapper {
		return ReaderWrapper { reader };
	}
	pub fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		return self.reader.get_tile_uncompressed(level, col, row);
	}
	pub fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		return self.reader.get_tile_raw(level, col, row);
	}
}

unsafe impl Send for ReaderWrapper<'_> {}