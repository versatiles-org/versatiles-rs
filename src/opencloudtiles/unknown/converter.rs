use crate::opencloudtiles;

pub struct Converter;
impl opencloudtiles::Converter for Converter {
	#![allow(unused_variables)]
	fn new(filename: &std::path::PathBuf) -> std::io::Result<Box<dyn opencloudtiles::Converter>>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: Box<dyn opencloudtiles::Reader>) -> std::io::Result<()> {
		panic!()
	}
	fn set_precompression(&mut self, compression: &opencloudtiles::TileCompression) {
		panic!()
	}
	fn set_minimum_zoom(&mut self, level: u64) {
		panic!()
	}
	fn set_maximum_zoom(&mut self, level: u64) {
		panic!()
	}
}
