use anyhow::{Result, bail};
use versatiles_container::{TilesConverterParameters, convert_tiles_container, get_reader};
use versatiles_core::{GeoBBox, TileBBoxPyramid, TileCompression, config::Config};
use versatiles_derive::context;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// supported container formats: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory
	#[arg()]
	input_file: String,

	/// supported container formats: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory
	#[arg()]
	output_file: String,

	/// minimum zoom level
	#[arg(long, value_name = "int", display_order = 1)]
	min_zoom: Option<u8>,

	/// maximum zoom level
	#[arg(long, value_name = "int", display_order = 1)]
	max_zoom: Option<u8>,

	/// use only tiles inside a bounding box
	#[arg(
		long,
		short,
		value_name = "lon_min,lat_min,lon_max,lat_max",
		allow_hyphen_values = true,
		display_order = 1
	)]
	bbox: Option<String>,

	/// also include additional tiles surrounding the bounding box as a border
	#[arg(long, value_name = "int", display_order = 1)]
	bbox_border: Option<u32>,

	/// set new compression
	#[arg(long, short, value_enum, display_order = 2)]
	compress: Option<TileCompression>,

	/// override the compression of the input source, e.g. to handle gzipped tiles in a tar, that do not end in .gz
	#[arg(long, value_enum, value_name = "COMPRESSION", display_order = 2)]
	override_input_compression: Option<TileCompression>,

	/// swap rows and columns, e.g. z/x/y -> z/y/x
	#[arg(long, display_order = 3)]
	swap_xy: bool,

	/// flip input vertically
	#[arg(long, display_order = 3)]
	flip_y: bool,

	/// set the output tile format
	#[arg(long, value_name = "TILE_FORMAT", display_order = 3)]
	tile_format: Option<versatiles_core::TileFormat>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	eprintln!("convert from {:?} to {:?}", arguments.input_file, arguments.output_file);

	let config = Config::default().arc();

	let mut reader = get_reader(&arguments.input_file, config.clone()).await?;

	if arguments.override_input_compression.is_some() {
		reader.override_compression(arguments.override_input_compression.unwrap());
	}

	let parameters = TilesConverterParameters {
		bbox_pyramid: get_bbox_pyramid(arguments)?,
		flip_y: arguments.flip_y,
		swap_xy: arguments.swap_xy,
	};

	let compression = arguments.compress.unwrap_or(reader.parameters().tile_compression);

	convert_tiles_container(reader, parameters, &arguments.output_file, compression, config).await?;

	eprintln!("finished converting tiles");

	Ok(())
}

#[context("Failed to get bounding box pyramid")]
fn get_bbox_pyramid(arguments: &Subcommand) -> Result<Option<TileBBoxPyramid>> {
	if arguments.min_zoom.is_none() && arguments.max_zoom.is_none() && arguments.bbox.is_none() {
		return Ok(None);
	}

	let mut bbox_pyramid = TileBBoxPyramid::new_full(32);

	if let Some(level_min) = arguments.min_zoom {
		bbox_pyramid.set_level_min(level_min)
	}

	if let Some(level_max) = arguments.max_zoom {
		bbox_pyramid.set_level_max(level_max)
	}

	if let Some(bbox) = &arguments.bbox {
		log::trace!("parsing bbox argument: {bbox:?}");
		let values: Vec<f64> = bbox
			.split(&[' ', ',', ';'])
			.filter(|s| !s.is_empty())
			.map(|s| s.parse::<f64>().expect("bbox value is not a number"))
			.collect();

		if values.len() != 4 {
			bail!("bbox must contain exactly 4 numbers, but instead i'v got: {bbox:?}");
		}

		bbox_pyramid.intersect_geo_bbox(&GeoBBox::try_from(values)?)?;

		if let Some(b) = arguments.bbox_border {
			bbox_pyramid.add_border(b, b, b, b);
		}
	}

	Ok(Some(bbox_pyramid))
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;
	use anyhow::Result;
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

	fn test_remote1() -> Result<()> {
		fs::create_dir("../tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=1",
			"--max-zoom=2",
			"--bbox=-180,-85,180,85",
			"--flip-y",
			"--force-recompress",
			"https://download.versatiles.org/osm.versatiles",
			"../tmp/planet2.versatiles",
		])?;
		Ok(())
	}

	#[test]

	fn test_remote2() -> Result<()> {
		fs::create_dir("../tmp/").unwrap_or_default();
		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=12",
			"--bbox=9.14,48.76,9.19,48.79",
			"--flip-y",
			"https://download.versatiles.org/osm.versatiles",
			"../tmp/stuttgart.versatiles",
		])?;
		Ok(())
	}
}
