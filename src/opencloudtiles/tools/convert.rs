use crate::{
	opencloudtiles::{
		container::{
			cloudtiles, mbtiles, tar_file, TileConverterBox, TileConverterTrait, TileReaderBox,
		},
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

fn new_reader(filename: &str, arguments: &Convert) -> TileReaderBox {
	let mut reader = get_reader(filename);

	reader
		.get_parameters_mut()
		.set_vertical_flip(arguments.flip_input);

	reader
}

fn new_converter(filename: &str, arguments: &Convert) -> TileConverterBox {
	let mut bbox_pyramide = TileBBoxPyramide::new_full();

	if let Some(value) = arguments.min_zoom {
		bbox_pyramide.set_zoom_min(value)
	}

	if let Some(value) = arguments.max_zoom {
		bbox_pyramide.set_zoom_max(value)
	}

	if let Some(value) = &arguments.bbox {
		bbox_pyramide.limit_by_geo_bbox(value.as_slice().try_into().unwrap());
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
		"tar" => tar_file::TileConverter::new(&path, config),
		_ => panic!("extension '{:?}' unknown", extension),
	};

	converter
}
