use crate::opencloudtiles::types::{
	Blob, Precompression, TileCoord3, TileFormat, TileReaderParameters,
};
use std::{fmt::Debug, path::PathBuf};

pub trait TileReaderTrait: Debug + Send + Sync {
	fn from_file(filename: &PathBuf) -> TileReaderBox
	where
		Self: Sized;
	fn get_name(&self) -> &str;
	fn get_parameters(&self) -> &TileReaderParameters;
	fn get_tile_format(&self) -> &TileFormat {
		self.get_parameters().get_tile_format()
	}
	fn get_tile_precompression(&self) -> &Precompression {
		self.get_parameters().get_tile_precompression()
	}

	/// always uncompressed
	fn get_meta(&self) -> Blob;

	/// always compressed with get_tile_precompression and formatted with get_tile_format
	fn get_tile_data(&mut self, coord: &TileCoord3) -> Option<Blob>;
}

pub type TileReaderBox = Box<dyn TileReaderTrait>;
