use crate::{
	opencloudtiles::{
		containers::{
			abstract_container::{TileConverter, TileReader},
			cloudtiles, mbtiles, tar, unknown,
		},
		types::TileConverterConfig,
	},
	Convert,
};
use std::path::PathBuf;

use super::types::TileBBoxPyramide;

pub struct Tools;
impl Tools {
	pub fn convert(command: &Convert) {
		let reader = Tools::new_reader(&command.input_file);
		let mut converter = Tools::new_converter(&command.output_file, command);
		converter.convert_from(reader);
	}
	fn new_reader(filename: &PathBuf) -> Box<dyn TileReader> {
		let extension = filename.extension().unwrap().to_str();
		let reader = match extension {
			Some("mbtiles") => mbtiles::TileReader::load(filename),
			Some("tar") => tar::TileReader::load(filename),
			Some("cloudtiles") => cloudtiles::TileReader::load(filename),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		return reader;
	}
	fn new_converter<'a>(filename: &'a PathBuf, command: &'a Convert) -> Box<dyn TileConverter> {
		let mut bbox_pyramide = TileBBoxPyramide::new_full();

		if command.min_zoom.is_some() {
			bbox_pyramide.set_zoom_min(command.min_zoom.unwrap())
		}

		if command.max_zoom.is_some() {
			bbox_pyramide.set_zoom_max(command.max_zoom.unwrap())
		}

		if command.bbox.is_some() {
			let array = command.bbox.as_ref().unwrap().as_slice();
			bbox_pyramide.limit_by_geo_bbox(array.try_into().unwrap());
		}

		let config = TileConverterConfig::new(
			command.tile_format.clone(),
			bbox_pyramide,
			command.force_recompress,
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
