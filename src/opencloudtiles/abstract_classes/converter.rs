#![allow(unused_variables)]

use std::path::PathBuf;

use super::{TileCompression, TileReader};

pub trait TileConverter {
	fn new(filename: &PathBuf) -> std::io::Result<Box<dyn TileConverter>>
	where
		Self: Sized;
	fn convert_from(&mut self, reader: Box<dyn TileReader>) -> std::io::Result<()>;
	fn set_minimum_zoom(&mut self, level: u64);
	fn set_maximum_zoom(&mut self, level: u64);
	fn set_precompression(&mut self, compression: &TileCompression);
}
