#![allow(unused_variables)]

use std::path::PathBuf;

use super::{TileCompression, TileFormat};

pub trait TileReader {
	fn load(filename: &PathBuf) -> std::io::Result<Box<dyn TileReader>>
	where
		Self: Sized,
	{
		panic!();
	}
	fn get_tile_format(&self) -> TileFormat {
		panic!();
	}
	fn get_tile_compression(&self) -> TileCompression {
		panic!();
	}
	fn get_meta(&self) -> &[u8] {
		panic!();
	}
	fn get_minimum_zoom(&self) -> u64 {
		panic!();
	}
	fn get_maximum_zoom(&self) -> u64 {
		panic!();
	}
	fn get_level_bbox(&self, level: u64) -> (u64, u64, u64, u64) {
		panic!();
	}
	fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		panic!();
	}
	fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		panic!();
	}
}

pub struct TileReaderWrapper<'a> {
	reader: &'a Box<dyn TileReader>,
}

impl TileReaderWrapper<'_> {
	pub fn new(reader: &Box<dyn TileReader>) -> TileReaderWrapper {
		return TileReaderWrapper { reader };
	}
	pub fn get_tile_uncompressed(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		return self.reader.get_tile_uncompressed(level, col, row);
	}
	pub fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Vec<u8>> {
		return self.reader.get_tile_raw(level, col, row);
	}
}

unsafe impl Send for TileReaderWrapper<'_> {}
unsafe impl Sync for TileReaderWrapper<'_> {}
