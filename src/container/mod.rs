mod abstract_classes;
mod cloudtiles;
mod mbtiles;
mod tar;

pub use abstract_classes::*;
use std::path::PathBuf;

use crate::Convert;

pub struct Tools;
impl Tools {
	pub fn convert(command: &Convert) -> std::io::Result<()> {
		let container_in = Tools::new_reader(&command.input_file, command)?;
		let mut converter = Tools::new_converter(&command.output_file, command)?;
		converter.convert_from(container_in)?;

		return Ok(());
	}
	pub fn new_reader(filename: &PathBuf, command: &Convert) -> std::io::Result<Box<dyn Reader>> {
		let extension = filename.extension().unwrap().to_str();
		let mut container = match extension {
			Some("mbtiles") => mbtiles::Reader::load(filename)?,
			Some("cloudtiles") => cloudtiles::Reader::load(filename)?,
			_ => panic!("extension '{:?}' unknown", extension),
		};

		if command.min_zoom.is_some() {
			let zoom: u64 = command.min_zoom.unwrap();
			if container.get_minimum_zoom() < zoom {
				container.set_minimum_zoom(zoom)
			}
		}

		if command.max_zoom.is_some() {
			let zoom: u64 = command.max_zoom.unwrap();
			if container.get_maximum_zoom() > zoom {
				container.set_maximum_zoom(zoom)
			}
		}

		return Ok(container);
	}
	pub fn new_converter(
		filename: &PathBuf,
		command: &Convert,
	) -> std::io::Result<Box<dyn Converter>> {
		let extension = filename.extension().unwrap().to_str();
		let mut converter = match extension {
			Some("mbtiles") => mbtiles::Converter::new(filename).unwrap(),
			Some("cloudtiles") => cloudtiles::Converter::new(filename).unwrap(),
			Some("tar") => tar::Converter::new(filename).unwrap(),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		if command.precompress.is_some() {
			converter.set_precompression(command.precompress.as_ref().unwrap());
		}

		return Ok(converter);
	}
}
