use crate::{container::*, helper::*, tools::get_reader};
use clap::Args;
use log::trace;
use std::path::PathBuf;

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// supported container formats: *.versatiles, *.tar, *.mbtiles
	#[arg()]
	input_file: String,

	/// supported container formats: *.versatiles, *.tar
	#[arg()]
	output_file: String,

	/// minimum zoom level
	#[arg(long, value_name = "int")]
	min_zoom: Option<u8>,

	/// maximum zoom level
	#[arg(long, value_name = "int")]
	max_zoom: Option<u8>,

	/// bounding box
	#[arg(
		long,
		short,
		value_name = "lon_min,lat_min,lon_max,lat_max",
		allow_hyphen_values = true
	)]
	bbox: Option<String>,

	/// flip input vertically
	#[arg(long)]
	flip_input: bool,

	/// convert tiles to new format
	#[arg(long, short, value_enum)]
	tile_format: Option<TileFormat>,

	/// set new precompression
	#[arg(long, short, value_enum)]
	precompress: Option<Precompression>,

	/// force recompression, e.g. to improve an existing gzip compression.
	#[arg(long, short, value_enum)]
	force_recompress: bool,
}

pub fn run(arguments: &Subcommand) {
	println!("convert from {:?} to {:?}", arguments.input_file, arguments.output_file);

	let mut reader = new_reader(&arguments.input_file, arguments);
	let mut converter = new_converter(&arguments.output_file, arguments);
	converter.convert_from(&mut reader);
}

fn new_reader(filename: &str, arguments: &Subcommand) -> TileReaderBox {
	let mut reader = get_reader(filename);

	reader.get_parameters_mut().set_vertical_flip(arguments.flip_input);

	reader
}

fn new_converter(filename: &str, arguments: &Subcommand) -> TileConverterBox {
	let mut bbox_pyramide = TileBBoxPyramide::new_full();

	if let Some(value) = arguments.min_zoom {
		bbox_pyramide.set_zoom_min(value)
	}

	if let Some(value) = arguments.max_zoom {
		bbox_pyramide.set_zoom_max(value)
	}

	if let Some(value) = &arguments.bbox {
		trace!("parsing bbox argument: {:?}", value);
		let values: Vec<f32> = value
			.split(&[' ', ',', ';'])
			.filter(|s| !s.is_empty())
			.map(|s| s.parse::<f32>().expect("bbox value is not a number"))
			.collect();
		if values.len() != 4 {
			panic!("bbox must contain exactly 4 numbers, but instead i'v got: {value:?}");
		}
		bbox_pyramide.limit_by_geo_bbox(values.as_slice().try_into().unwrap());
	}

	let config = TileConverterConfig::new(
		arguments.tile_format.clone(),
		arguments.precompress,
		bbox_pyramide,
		arguments.force_recompress,
	);

	let path = PathBuf::from(filename);
	let extension = path.extension().unwrap().to_str().unwrap();

	let converter = match extension {
		"mbtiles" => mbtiles::TileConverter::new(&path, config),
		"versatiles" => versatiles::TileConverter::new(&path, config),
		"tar" => tar_file::TileConverter::new(&path, config),
		_ => panic!("extension '{extension:?}' unknown"),
	};

	converter
}
