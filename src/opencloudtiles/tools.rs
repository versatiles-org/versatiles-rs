use crate::{
	opencloudtiles::{
		abstract_classes::{self, *},
		cloudtiles, mbtiles, tar,
		types::TileConverterConfig,
	},
	Convert,
};
use std::path::PathBuf;

use super::unknown;

pub struct Tools;
impl Tools {
	pub fn convert(command: &Convert) {
		let reader = Tools::new_reader(&command.input_file);
		let mut converter = Tools::new_converter(&command.output_file, command);
		converter.convert_from(reader);
	}
	fn new_reader(filename: &PathBuf) -> Box<dyn abstract_classes::TileReader> {
		let extension = filename.extension().unwrap().to_str();
		let reader = match extension {
			Some("mbtiles") => mbtiles::TileReader::load(filename),
			Some("tar") => tar::TileReader::load(filename),
			Some("cloudtiles") => cloudtiles::TileReader::load(filename),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		return reader;
	}
	fn new_converter<'a>(
		filename: &'a PathBuf,
		command: &'a Convert,
	) -> Box<dyn abstract_classes::TileConverter> {
		let config = TileConverterConfig::from_options(
			&command.min_zoom,
			&command.max_zoom,
			&command.bbox,
			&command.tile_format,
			&command.force_recompress,
		);

		let extension = filename.extension().unwrap().to_str();

		let converter = match extension {
			Some("mbtiles") => mbtiles::TileConverter::new(filename, config),
			Some("cloudtiles") => cloudtiles::TileConverter::new(filename, config),
			Some("tar") => tar::TileConverter::new(filename, config),
			Some("*") => unknown::TileConverter::new(filename, config),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		return converter;
	}
}
