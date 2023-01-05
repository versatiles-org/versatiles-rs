use crate::opencloudtiles::types::{
	Blob, Precompression, TileConverterConfig, TileCoord3, TileFormat, TileReaderParameters,
};
use std::fmt::Debug;
use std::path::PathBuf;

pub trait TileConverterTrait {
	fn new(filename: &PathBuf, config: TileConverterConfig) -> Box<dyn TileConverterTrait>
	where
		Self: Sized;

	// readers must be mutable, because they might use caching
	fn convert_from(&mut self, reader: &mut TileReaderBox);
}

pub trait TileReaderTrait: Debug + Send + Sync {
	fn new(filename: &str) -> TileReaderBox
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
	fn get_tile_data(&self, coord: &TileCoord3) -> Option<Blob>;
}

pub type TileReaderBox = Box<dyn TileReaderTrait>;
