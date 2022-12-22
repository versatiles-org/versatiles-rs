#![allow(unused_variables)]

use std::path::PathBuf;

use super::{Reader, TileCompression};

pub trait Converter {
	fn new(filename: &PathBuf) -> std::io::Result<Box<dyn Converter>>
	where
		Self: Sized;
	fn convert_from(&mut self, reader: Box<dyn Reader>) -> std::io::Result<()>;
	fn set_minimum_zoom(&mut self, level: u64);
	fn set_maximum_zoom(&mut self, level: u64);
	fn set_precompression(&mut self, compression: &TileCompression);
}
