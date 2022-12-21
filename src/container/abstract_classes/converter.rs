#![allow(unused_variables)]

use std::path::PathBuf;

use super::{Reader, TileCompression};

pub trait Converter {
	fn new(filename: &PathBuf) -> std::io::Result<Box<dyn Converter>>
	where
		Self: Sized,
	{
		panic!("not implemented: new");
	}
	fn convert_from(&mut self, container: Box<dyn Reader>) -> std::io::Result<()> {
		panic!("not implemented: convert_from");
	}
	fn set_precompression(&mut self, compression: &TileCompression) {
		panic!("not implemented: set_precompression");
	}
}
