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
	fn new_reader<'a>(
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
	fn new_converter<'a>(
		filename: &'a PathBuf,
		command: &'a Convert,
	) -> Result<Box<dyn TileConverter>, &'static str> {
		let config = TileConverterConfig::from_options(
			&command.min_zoom,
			&command.max_zoom,
			&command.tile_format,
			&command.force_recompress,
		);

		let extension = filename.extension().unwrap().to_str();

		let converter = match extension {
			Some("mbtiles") => mbtiles::TileConverter::new(filename, config).unwrap(),
			Some("cloudtiles") => cloudtiles::TileConverter::new(filename, config).unwrap(),
			Some("tar") => tar::TileConverter::new(filename, config).unwrap(),
			Some("*") => unknown::TileConverter::new(filename, config).unwrap(),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		return Ok(converter);
	}
}
