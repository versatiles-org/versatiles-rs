use crate::container::abstract_classes;

pub struct Converter;
impl abstract_classes::Converter for Converter {
	#![allow(unused_variables)]
	fn new(filename: &std::path::PathBuf) -> std::io::Result<Box<dyn abstract_classes::Converter>>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: Box<dyn abstract_classes::Reader>) -> std::io::Result<()> {
		panic!()
	}
	fn set_precompression(&mut self, compression: &abstract_classes::TileCompression) {
		panic!()
	}
	fn set_minimum_zoom(&mut self, level: u64) {
		panic!()
	}
	fn set_maximum_zoom(&mut self, level: u64) {
		panic!()
	}
}
