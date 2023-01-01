#![allow(unused_variables)]

use std::path::PathBuf;

use crate::opencloudtiles::types::TileConverterConfig;

use super::TileReaderBox;

pub trait TileConverter {
	fn new(filename: &PathBuf, config: TileConverterConfig) -> Box<dyn TileConverter>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: &mut TileReaderBox) {
		panic!()
	}
}
