#![allow(unused_variables)]

use std::path::PathBuf;

use crate::opencloudtiles::abstract_classes;
use crate::opencloudtiles::types::TileConverterConfig;

pub trait TileConverter {
	fn new(filename: &PathBuf, config: TileConverterConfig) -> Box<dyn TileConverter>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: Box<dyn abstract_classes::TileReader>) {
		panic!()
	}
}
