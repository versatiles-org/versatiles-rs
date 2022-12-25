#![allow(unused_variables)]

use crate::opencloudtiles::types::{TileData, TileReaderParameters};
use std::path::PathBuf;

pub trait TileReader {
	fn load(filename: &PathBuf) -> Box<dyn TileReader>
	where
		Self: Sized,
	{
		panic!();
	}
	fn get_meta(&self) -> &[u8] {
		panic!();
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		panic!();
	}
	fn get_tile_data(&self, level: u64, col: u64, row: u64) -> Option<TileData> {
		panic!();
	}
}
