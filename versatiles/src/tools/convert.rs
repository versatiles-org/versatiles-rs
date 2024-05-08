use crate::libs::TileCompression;
use anyhow::{bail, Result};
use clap::Args;
use log::trace;
use versatiles_lib::{
	containers::{convert_tiles_container, get_reader, TilesConverterParameters},
	shared::{TileBBoxPyramid, TileFormat},
};

#[derive(Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// supported container formats: *.versatiles, *.tar, *.mbtiles or a directory
	#[arg()]
	input_file: String,

	/// supported container formats: *.versatiles, *.tar or a directory
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

	/// also include additional tiles surrounding the bounding box as a border
	#[arg(long, value_name = "int")]
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
	compress: Option<TileCompression>,

	/// force recompression, e.g. to improve an existing gzip compression
	#[arg(long, short)]
	force_recompress: bool,

	/// override the compression of the input source, e.g. to handle gzipped tiles in a tar, that do not end in .gz
	#[arg(long, value_enum, value_name = "COMPRESSION")]
	override_input_compression: Option<TileCompression>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	eprintln!("convert from {:?} to {:?}", arguments.input_file, arguments.output_file);

	let mut reader = get_reader(&arguments.input_file).await?;

	if arguments.override_input_compression.is_some() {
		reader.override_compression(arguments.override_input_compression.as_ref().unwrap().to_value());
	}

	let cp = TilesConverterParameters::new(
		arguments.tile_format,
		arguments.compress.as_ref().map(|c| c.to_value()),
		get_bbox_pyramid(arguments)?,
		arguments.force_recompress,
		arguments.flip_y,
		arguments.swap_xy,
	);
	convert_tiles_container(reader, cp, &arguments.output_file).await?;

	Ok(())
}

fn get_bbox_pyramid(arguments: &Subcommand) -> Result<Option<TileBBoxPyramid>> {
	if arguments.min_zoom.is_none() && arguments.max_zoom.is_none() && arguments.bbox.is_none() {
		return Ok(None);
	}

	let mut bbox_pyramid = TileBBoxPyramid::new_full(32);

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
			bail!("bbox must contain exactly 4 numbers, but instead i'v got: {bbox:?}");
		}

		bbox_pyramid.intersect_geo_bbox(values.as_slice().try_into()?);

		if let Some(b) = arguments.bbox_border {
			bbox_pyramid.add_border(b, b, b, b);
		}
	}

	Ok(Some(bbox_pyramid))
}

#[allow(unused_imports)]
#[cfg(test)]
mod tests {
	use anyhow::Result;

	use crate::tests::run_command;
	use std::fs;

	#[test]
	fn test_local() -> Result<()> {
		fs::create_dir("../tmp/").unwrap_or_default();

		run_command(vec![
			"versatiles",
			"convert",
			"../testdata/berlin.mbtiles",
			"../tmp/berlin1.versatiles",
		])?;

		run_command(vec![
			"versatiles",
			"convert",
			"--bbox=13.38,52.46,13.43,52.49",
			"../tmp/berlin1.versatiles",
			"../tmp/berlin2.versatiles",
		])?;

		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=1",
			"--max-zoom=13",
			"--flip-y",
			"--force-recompress",
			"../tmp/berlin2.versatiles",
			"../tmp/berlin3.versatiles",
		])?;

		Ok(())
	}

	#[test]

	fn test_remote1() {
		fs::create_dir("../tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=1",
			"--max-zoom=3",
			"--bbox=-180,-85,180,85",
			"--flip-y",
			"--force-recompress",
			"https://download.versatiles.org/osm.versatiles",
			"../tmp/planet2.versatiles",
		])
		.unwrap();
	}

	#[test]

	fn test_remote2() {
		fs::create_dir("../tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=12",
			"--bbox=9.14,48.76,9.19,48.79",
			"--flip-y",
			"https://download.versatiles.org/osm.versatiles",
			"../tmp/stuttgart.versatiles",
		])
		.unwrap();
	}
}
