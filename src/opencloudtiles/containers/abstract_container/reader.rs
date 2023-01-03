use crate::opencloudtiles::types::{TileCoord3, TileData, TileReaderParameters};
use std::{fmt::Debug, path::PathBuf};

pub trait TileReaderTrait: Debug + Send + Sync {
	fn from_file(filename: &PathBuf) -> TileReaderBox
	where
		Self: Sized;
	fn get_meta(&self) -> &[u8];
	fn get_name(&self) -> &str;
	fn get_parameters(&self) -> &TileReaderParameters;
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Option<TileData>;
}

pub type TileReaderBox = Box<dyn TileReaderTrait>;
