mod container;
mod cloudtiles;
mod mbtiles;

use std::path::PathBuf;
use crate::tiles::container::{Converter,Reader};


pub struct Tiles;
impl Tiles {
	pub fn convert(filename_in: &PathBuf, filename_out: &PathBuf) -> Result<String, String> {
		let container_in = Tiles::new_reader(filename_in);
		return Tiles::convert_from(filename_out, &container_in);
	}
	pub fn new_reader(filename: &PathBuf) -> impl container::Reader {
		let extension = filename.extension().unwrap().to_str();
		match extension {
			Some("mbtiles") => mbtiles::Reader::load(filename),
			_ => panic!("extension '{:?}' unknown", extension),
		}
	}
	pub fn convert_from(filename: &PathBuf, reader: &impl container::Reader) -> Result<String, String> {
		let extension = filename.extension().unwrap().to_str();
		match extension {
			Some("mbtiles") => mbtiles::Converter::convert_from(filename, reader),
			_ => panic!("extension '{:?}' unknown", extension),
		}
	}
}
