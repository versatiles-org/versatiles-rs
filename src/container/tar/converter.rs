use crate::container;

pub struct Converter;
impl container::Converter for Converter {
	#![allow(unused_variables)]
	fn new(filename: &std::path::PathBuf) -> std::io::Result<Box<dyn container::Converter>>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: Box<dyn container::Reader>) -> std::io::Result<()> {
		panic!()
	}
	fn set_precompression(&mut self, compression: &container::TileCompression) {
		panic!()
	}
	fn set_minimum_zoom(&mut self, level: u64) {
		panic!()
	}
	fn set_maximum_zoom(&mut self, level: u64) {
		panic!()
	}
}
