use crate::{
	opencloudtiles::{
		container::{cloudtiles, mbtiles, tar, TileConverterTrait, TileConverterBox},
		lib::{TileBBoxPyramide, TileConverterConfig}, tools::get_reader,
	}, Convert,
};
use std::path::PathBuf;

pub fn convert(arguments: &Convert) {
	println!(
		"convert from {:?} to {:?}",
		arguments.input_file, arguments.output_file
	);

	let mut reader = get_reader(&arguments.input_file);
	let mut converter = new_converter(&arguments.output_file, arguments);
	converter.convert_from(&mut reader);
}

fn new_converter(filename: &str, command: &Convert) -> TileConverterBox {
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
		command.precompress,
		bbox_pyramide,
		command.force_recompression,
	);

	let path = PathBuf::from(filename);
	let extension = path.extension().unwrap().to_str().unwrap();

	let converter = match extension {
		"mbtiles" => mbtiles::TileConverter::new(&path, config),
		"cloudtiles" => cloudtiles::TileConverter::new(&path, config),
		"tar" => tar::TileConverter::new(&path, config),
		_ => panic!("extension '{:?}' unknown", extension),
	};

	converter
}
