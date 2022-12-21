mod abstract_classes;
mod cloudtiles;
mod mbtiles;

pub use abstract_classes::*;
use std::path::PathBuf;

use crate::Cli;

pub struct Tiles;
impl Tiles {
	pub fn convert(filename_in: &PathBuf, filename_out: &PathBuf, cli: &Cli) -> std::io::Result<()> {
		let container_in = Tiles::new_reader(filename_in, cli)?;
		let mut converter = Tiles::new_converter(filename_out, cli)?;
		converter.convert_from(container_in)?;

		return Ok(());
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
	pub fn new_converter(filename: &PathBuf, cli: &Cli) -> std::io::Result<Box<dyn Converter>> {
		let extension = filename.extension().unwrap().to_str();
		let mut converter = match extension {
			Some("mbtiles") => mbtiles::Converter::new(filename).unwrap(),
			Some("cloudtiles") => cloudtiles::Converter::new(filename).unwrap(),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		if cli.precompress.is_some() {
			converter.set_precompression(cli.precompress.as_ref().unwrap());
		}

		return Ok(converter);
	}
}
