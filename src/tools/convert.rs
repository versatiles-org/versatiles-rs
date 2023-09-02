use crate::{
	containers::{get_converter, get_reader, TileConverterBox, TileReaderBox},
	create_error,
	shared::{Compression, Result, TileBBoxPyramid, TileConverterConfig, TileFormat},
};
use clap::Args;
use log::trace;

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
	bbox_border: Option<u32>,

	/// swap rows and columns, e.g. z/x/y -> z/y/x
	#[arg(long)]
	swap_xy: bool,

	/// flip input vertically
	#[arg(long)]
	flip_y: bool,

	/// convert tiles to new format
	#[arg(long, short, value_enum)]
	tile_format: Option<TileFormat>,

	/// set new compression
	#[arg(long, short, value_enum)]
	compress: Option<Compression>,

	/// force recompression, e.g. to improve an existing gzip compression
	#[arg(long, short)]
	force_recompress: bool,

	/// override the compression of the input source, e.g. to handle gzipped tiles in a tar, that do not end in .gz
	#[arg(long, value_enum, value_name = "COMPRESSION")]
	override_input_compression: Option<Compression>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	eprintln!("convert from {:?} to {:?}", arguments.input_file, arguments.output_file);

	let mut reader = new_reader(&arguments.input_file, arguments).await?;
	let mut converter = new_converter(&arguments.output_file, arguments).await?;

	converter.convert_from(&mut reader).await
}

async fn new_reader(filename: &str, arguments: &Subcommand) -> Result<TileReaderBox> {
	let mut reader = get_reader(filename).await?;
	let parameters = reader.get_parameters_mut()?;

	parameters.set_swap_xy(arguments.swap_xy);
	parameters.set_flip_y(arguments.flip_y);

	if let Some(compression) = arguments.override_input_compression {
		parameters.set_tile_compression(compression);
	}

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
		let values: Vec<f64> = bbox
			.split(&[' ', ',', ';'])
			.filter(|s| !s.is_empty())
			.map(|s| s.parse::<f64>().expect("bbox value is not a number"))
			.collect();

		if values.len() != 4 {
			return create_error!("bbox must contain exactly 4 numbers, but instead i'v got: {bbox:?}");
		}

		bbox_pyramid.intersect_geo_bbox(values.as_slice().try_into()?);

		if let Some(b) = arguments.bbox_border {
			bbox_pyramid.add_border(b, b, b, b);
		}
	}

	let config = TileConverterConfig::new(
		arguments.tile_format.clone(),
		arguments.compress,
		bbox_pyramid,
		arguments.force_recompress,
	);

	let converter = get_converter(filename, config).await?;

	Ok(converter)
}

#[allow(unused_imports)]
#[cfg(test)]
mod tests {
	use crate::tests::run_command;
	use std::fs;

	#[cfg(feature = "mbtiles")]
	#[test]
	fn test_local() {
		fs::create_dir("tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"testdata/berlin.mbtiles",
			"tmp/berlin1.versatiles",
		])
		.unwrap();
		run_command(vec![
			"versatiles",
			"convert",
			"--flip-y",
			"tmp/berlin1.versatiles",
			"tmp/berlin2.versatiles",
		])
		.unwrap();
		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=1",
			"--max-zoom=13",
			"--bbox=13.38,52.46,13.43,52.49",
			"--flip-y",
			"--force-recompress",
			"tmp/berlin2.versatiles",
			"tmp/berlin3.versatiles",
		])
		.unwrap();
	}

	#[test]
	#[cfg(feature = "request")]
	fn test_remote1() {
		fs::create_dir("tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=1",
			"--max-zoom=3",
			"--bbox=-85,-180,85,180",
			"--flip-y",
			"--force-recompress",
			"https://download.versatiles.org/planet-latest.versatiles",
			"tmp/planet2.versatiles",
		])
		.unwrap();
	}

	#[test]
	#[cfg(feature = "request")]
	fn test_remote2() {
		fs::create_dir("tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=12",
			"--bbox=9.14,48.76,9.19,48.79",
			"--flip-y",
			"https://download.versatiles.org/planet-latest.versatiles",
			"tmp/stuttgart.versatiles",
		])
		.unwrap();
	}
}
