use crate::{
	opencloudtiles::{
		container::{cloudtiles, mbtiles, tar, TileConverterBox, TileConverterTrait, TileReaderBox},
		lib::{TileBBoxPyramide, TileConverterConfig},
		tools::get_reader,
	},
	Convert,
};
use std::path::PathBuf;

pub fn convert(arguments: &Convert) {
	println!(
		"convert from {:?} to {:?}",
		arguments.input_file, arguments.output_file
	);

	let mut reader = new_reader(&arguments.input_file, arguments);
	let mut converter = new_converter(&arguments.output_file, arguments);
	converter.convert_from(&mut reader);
}

fn new_reader(filename:&str, arguments: &Convert) -> TileReaderBox {
	let mut reader = get_reader(filename);

	reader.get_parameters_mut().set_vertical_flip(arguments.flip_input);

	return reader;
}

fn new_converter(filename:&str, arguments: &Convert) -> TileConverterBox {
	let mut bbox_pyramide = TileBBoxPyramide::new_full();

	if arguments.min_zoom.is_some() {
		bbox_pyramide.set_zoom_min(arguments.min_zoom.unwrap())
	}

	if arguments.max_zoom.is_some() {
		bbox_pyramide.set_zoom_max(arguments.max_zoom.unwrap())
	}

	if arguments.bbox.is_some() {
		let array = arguments.bbox.as_ref().unwrap().as_slice();
		bbox_pyramide.limit_by_geo_bbox(array.try_into().unwrap());
	}

	let config = TileConverterConfig::new(
		arguments.tile_format.clone(),
		arguments.precompress,
		bbox_pyramide,
		arguments.force_recompression,
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
