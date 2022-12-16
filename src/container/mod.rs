mod cloudtiles;
mod container;
mod mbtiles;

use container::{Converter, Reader};
use std::path::PathBuf;

pub struct Tiles;
impl Tiles {
	pub fn convert(filename_in: &PathBuf, filename_out: &PathBuf) -> std::io::Result<()> {
		let container_in = Tiles::new_reader(filename_in);
		return Tiles::convert_from(filename_out, container_in);
	}
	pub fn new_reader(filename: &PathBuf) -> Box<dyn Reader> {
		let extension = filename.extension().unwrap().to_str();
		match extension {
			Some("mbtiles") => return mbtiles::Reader::load(filename),
			Some("cloudtiles") => return cloudtiles::Reader::load(filename),
			_ => panic!("extension '{:?}' unknown", extension),
		};
	}
	pub fn convert_from(filename: &PathBuf, reader: Box<dyn Reader>) -> std::io::Result<()> {
		let extension = filename.extension().unwrap().to_str();
		match extension {
			Some("mbtiles") => mbtiles::Converter::convert_from(filename, reader),
			Some("cloudtiles") => cloudtiles::Converter::convert_from(filename, reader),
			_ => panic!("extension '{:?}' unknown", extension),
		}
	}
}
