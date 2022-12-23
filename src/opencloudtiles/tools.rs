use super::{abstract_classes::*, *};
use crate::Convert;
use std::path::PathBuf;

pub struct Tools;
impl Tools {
	pub fn convert(command: &Convert) -> Result<(), &'static str> {
		let reader = Tools::new_reader(&command.input_file, command)?;
		let mut converter = Tools::new_converter(&command.output_file, command)?;
		converter.convert_from(reader)?;

		return Ok(());
	}
	pub fn new_reader<'a>(
		filename: &'a PathBuf,
		_command: &'a Convert,
	) -> Result<Box<dyn TileReader>, &'static str> {
		let extension = filename.extension().unwrap().to_str();
		let reader = match extension {
			Some("mbtiles") => mbtiles::TileReader::load(filename).unwrap(),
			Some("cloudtiles") => cloudtiles::TileReader::load(filename).unwrap(),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		return Ok(reader);
	}
	pub fn new_converter<'a>(
		filename: &'a PathBuf,
		command: &'a Convert,
	) -> Result<Box<dyn TileConverter>, &'static str> {
		let mut config = TileConverterConfig::new_empty();
		config.set_min_zoom(&command.min_zoom);
		config.set_max_zoom(&command.max_zoom);
		config.set_tile_format(&command.tile_format);
		config.set_recompress(&command.force_recompress);

		let extension = filename.extension().unwrap().to_str();

		let converter = match extension {
			Some("mbtiles") => mbtiles::TileConverter::new(filename, Some(config)).unwrap(),
			Some("cloudtiles") => cloudtiles::TileConverter::new(filename, Some(config)).unwrap(),
			Some("tar") => tar::TileConverter::new(filename, Some(config)).unwrap(),
			Some("*") => unknown::TileConverter::new(filename, Some(config)).unwrap(),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		return Ok(converter);
	}
}
