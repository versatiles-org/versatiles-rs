use crate::{
	containers::{get_converter, get_reader, TileConverterBox, TileReaderBox},
	shared::{Compression, Error, Result, TileBBoxPyramid, TileConverterConfig, TileFormat},
};
use clap::Args;
use log::{error, trace};

#[derive(Args, Debug)]
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

	/// use only tiles inside a bounding box
	#[arg(
		long,
		short,
		value_name = "lon_min,lat_min,lon_max,lat_max",
		allow_hyphen_values = true
	)]
	bbox: Option<String>,

	/// also use tiles surrounding the bounding box as an additional border
	#[arg(long)]
	bbox_border: Option<u64>,

	/// flip input vertically
	#[arg(long)]
	flip_input: bool,

	/// convert tiles to new format
	#[arg(long, short, value_enum)]
	tile_format: Option<TileFormat>,

	/// set new compression
	#[arg(long, short, value_enum)]
	precompress: Option<Compression>,

	/// force recompression, e.g. to improve an existing gzip compression.
	#[arg(long, short)]
	force_recompress: bool,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	println!("convert from {:?} to {:?}", arguments.input_file, arguments.output_file);

	let mut reader = new_reader(&arguments.input_file, arguments).await?;
	let mut converter = new_converter(&arguments.output_file, arguments).await?;

	converter.convert_from(&mut reader).await
}

async fn new_reader(filename: &str, arguments: &Subcommand) -> Result<TileReaderBox> {
	let mut reader = get_reader(filename).await?;

	reader.get_parameters_mut()?.set_vertical_flip(arguments.flip_input);

	Ok(reader)
}

async fn new_converter(filename: &str, arguments: &Subcommand) -> Result<TileConverterBox> {
	let mut bbox_pyramid = TileBBoxPyramid::new_full();

	if let Some(min_zoom) = arguments.min_zoom {
		bbox_pyramid.set_zoom_min(min_zoom)
	}

	if let Some(max_zoom) = arguments.max_zoom {
		bbox_pyramid.set_zoom_max(max_zoom)
	}

	if let Some(bbox) = &arguments.bbox {
		trace!("parsing bbox argument: {:?}", bbox);
		let values: Vec<f32> = bbox
			.split(&[' ', ',', ';'])
			.filter(|s| !s.is_empty())
			.map(|s| s.parse::<f32>().expect("bbox value is not a number"))
			.collect();

		if values.len() != 4 {
			error!("bbox must contain exactly 4 numbers, but instead i'v got: {bbox:?}");
			return Err(Error::new("bbox must contain exactly 4 numbers"));
		}

		bbox_pyramid.intersect_geo_bbox(values.as_slice().try_into()?);

		if let Some(b) = arguments.bbox_border {
			bbox_pyramid.add_border(&b, &b, &b, &b);
		}
	}

	let config = TileConverterConfig::new(
		arguments.tile_format.clone(),
		arguments.precompress,
		bbox_pyramid,
		arguments.force_recompress,
	);

	let converter = get_converter(filename, config).await?;

	Ok(converter)
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;
	use std::fs;

	#[test]
	fn test_local() {
		fs::create_dir("tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"resources/berlin.mbtiles",
			"tmp/berlin1.versatiles",
		])
		.unwrap();
	}

	#[test]
	fn test_remote() {
		fs::create_dir("tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom",
			"1",
			"--max-zoom",
			"3",
			"--bbox",
			"-85,-180,85,180",
			"--flip-input",
			"--force-recompress",
			"https://download.versatiles.org/planet-20230227.versatiles",
			"tmp/berlin2.versatiles",
		])
		.unwrap();
	}
}
