use crate::opencloudtiles::types::{Blob, Precompression, TileCoord3, TileReaderParameters};
use std::{fmt::Debug, path::PathBuf};

pub trait TileReaderTrait: Debug + Send + Sync {
	fn from_file(filename: &PathBuf) -> TileReaderBox
	where
		Self: Sized;
	fn get_name(&self) -> &str;
	fn get_parameters(&self) -> &TileReaderParameters;
	fn get_meta(&self) -> (Blob, Precompression);
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Option<(Blob, Precompression)>;
}

pub type TileReaderBox = Box<dyn TileReaderTrait>;
