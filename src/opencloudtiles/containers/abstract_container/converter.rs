#![allow(unused_variables)]

use super::TileReaderBox;
use crate::opencloudtiles::types::TileConverterConfig;
use std::path::PathBuf;

pub trait TileConverterTrait {
	fn new(filename: &PathBuf, config: TileConverterConfig) -> Box<dyn TileConverterTrait>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: &mut TileReaderBox) {
		panic!()
	}
}
