use anyhow::{Result, bail};
use std::{path::PathBuf, sync::Arc};
use versatiles_container::{TilesConverterParameters, TilesRuntime, convert_tiles_container};
use versatiles_core::{GeoBBox, TileBBoxPyramid, TileCompression};
use versatiles_derive::context;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// supported container formats: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory
	#[arg()]
	input_file: String,

	/// supported container formats: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory
	#[arg()]
	output_file: PathBuf,

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
pub async fn run(arguments: &Subcommand, runtime: Arc<TilesRuntime>) -> Result<()> {
	log::info!("convert from {:?} to {:?}", arguments.input_file, arguments.output_file);

	let mut reader = runtime.registry().get_reader_from_str(&arguments.input_file).await?;

	if arguments.override_input_compression.is_some() {
		reader.override_compression(arguments.override_input_compression.unwrap());
	}

	let parameters = TilesConverterParameters {
		bbox_pyramid: get_bbox_pyramid(arguments)?,
		flip_y: arguments.flip_y,
		swap_xy: arguments.swap_xy,
		tile_compression: arguments.compress,
	};

	convert_tiles_container(reader, parameters, &arguments.output_file, runtime).await?;

	log::info!("finished converting tiles");

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
	use assert_fs::TempDir;

	#[test]
	fn test_local() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let temp_path = temp_dir.path().display();

		println!("{:?}", std::time::SystemTime::now());
		run_command(vec![
			"versatiles",
			"convert",
			"../testdata/berlin.mbtiles",
			&format!("{temp_path}/berlin1.versatiles"),
		])?;
		println!("{:?}", std::time::SystemTime::now());

		run_command(vec![
			"versatiles",
			"convert",
			"--bbox=13.38,52.46,13.43,52.49",
			&format!("{temp_path}/berlin1.versatiles"),
			&format!("{temp_path}/berlin2.versatiles"),
		])?;
		println!("{:?}", std::time::SystemTime::now());

		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=1",
			"--max-zoom=13",
			"--flip-y",
			&format!("{temp_path}/berlin2.versatiles"),
			&format!("{temp_path}/berlin3.versatiles"),
		])?;
		println!("{:?}", std::time::SystemTime::now());

		run_command(vec![
			"versatiles",
			"convert",
			"../testdata/berlin.vpl",
			&format!("{temp_path}/berlin4.pmtiles"),
		])?;
		println!("{:?}", std::time::SystemTime::now());

		Ok(())
	}

	#[test]
	fn test_remote1() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let temp_path = temp_dir.path().display();

		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=1",
			"--max-zoom=2",
			"--bbox=-180,-85,180,85",
			"--flip-y",
			"https://download.versatiles.org/osm.versatiles",
			&format!("{temp_path}/planet2.versatiles"),
		])?;
		Ok(())
	}

	#[test]
	fn test_remote2() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let temp_path = temp_dir.path().display();

		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=12",
			"--bbox=9.14,48.76,9.19,48.79",
			"--flip-y",
			"https://download.versatiles.org/osm.versatiles",
			&format!("{temp_path}/stuttgart.versatiles"),
		])?;
		Ok(())
	}
}
