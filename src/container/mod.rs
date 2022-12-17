mod cloudtiles;
mod container;
mod mbtiles;

use container::{Converter, Reader};
use std::path::PathBuf;

use crate::Cli;

pub struct Tiles;
impl Tiles {
	pub fn convert(filename_in: &PathBuf, filename_out: &PathBuf, cli: &Cli) -> std::io::Result<()> {
		let container_in = Tiles::new_reader(filename_in, cli)?;

		return Tiles::convert_from(filename_out, container_in);
	}
	pub fn new_reader(filename: &PathBuf, cli: &Cli) -> std::io::Result<Box<dyn Reader>> {
		let extension = filename.extension().unwrap().to_str();
		let mut container = match extension {
			Some("mbtiles") => mbtiles::Reader::load(filename)?,
			Some("cloudtiles") => cloudtiles::Reader::load(filename)?,
			_ => panic!("extension '{:?}' unknown", extension),
		};

		if cli.min_zoom.is_some() {
			let zoom: u64 = cli.min_zoom.unwrap();
			if container.get_minimum_zoom() < zoom {
				container.set_minimum_zoom(zoom)
			}
		}

		if cli.max_zoom.is_some() {
			let zoom: u64 = cli.max_zoom.unwrap();
			if container.get_maximum_zoom() > zoom {
				container.set_maximum_zoom(zoom)
			}
		}

		return Ok(container);
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
