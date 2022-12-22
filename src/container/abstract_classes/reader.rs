#![allow(unused_variables)]

use std::path::PathBuf;

use super::{TileCompression, TileFormat};

pub trait Reader {
	fn load(filename: &PathBuf) -> std::io::Result<Box<dyn Reader>>
	where
		Self: Sized;
	fn get_tile_format(&self) -> TileFormat;
	fn get_tile_compression(&self) -> TileCompression;
	fn get_meta(&self) -> &[u8];
	fn get_minimum_zoom(&self) -> u64;
	fn get_maximum_zoom(&self) -> u64;
	fn get_level_bbox(&self, level: u64) -> (u64, u64, u64, u64);
	fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>>;
	fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>>;
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
unsafe impl Sync for ReaderWrapper<'_> {}
