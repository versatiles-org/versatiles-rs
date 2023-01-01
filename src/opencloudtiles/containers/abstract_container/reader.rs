#![allow(unused_variables)]

use crate::opencloudtiles::types::{TileCoord3, TileData, TileReaderParameters};
use std::path::PathBuf;

pub trait TileReaderTrait {
	fn from_file(filename: &PathBuf) -> TileReaderBox
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
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Option<TileData> {
		panic!();
	}
}

pub type TileReaderBox = Box<dyn TileReaderTrait + Send + Sync>;
