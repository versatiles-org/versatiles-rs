#![allow(unused_variables)]

use crate::opencloudtiles::types::{TileCoord3, TileData, TileReaderParameters};
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
	fn get_tile_data(&self, coord: &TileCoord3) -> Option<TileData> {
		panic!();
	}
}
