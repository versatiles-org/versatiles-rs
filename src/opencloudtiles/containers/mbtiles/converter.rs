use std::path::PathBuf;

use crate::opencloudtiles::{
	containers::{TileConverterTrait, TileReaderBox},
	types::TileConverterConfig,
};

pub struct TileConverter;
impl TileConverterTrait for TileConverter {
	fn new(_filename: &PathBuf, _config: TileConverterConfig) -> Box<dyn TileConverterTrait>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, _reader: &mut TileReaderBox) {
		panic!()
	}
}
