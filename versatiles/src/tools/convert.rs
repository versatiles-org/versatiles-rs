use anyhow::{Result, bail};
use versatiles_container::{TilesConverterParameters, TilesRuntime, convert_tiles_container_to_str};
use versatiles_core::{GeoBBox, TileCompression, TileFormat, TilePyramid};
use versatiles_derive::context;

/// Parse a tile format string like "webp", "webp,80", or "avif,90,50"
/// into (TileFormat, Option<quality>, Option<effort>).
fn parse_tile_format(s: &str) -> Result<(TileFormat, Option<u8>, Option<u8>)> {
	let parts: Vec<&str> = s.split(',').collect();
	if parts.is_empty() || parts.len() > 3 {
		bail!("Invalid tile format '{s}': expected format[,quality][,effort]");
	}

	let format = TileFormat::try_from_str(parts[0])?;
	if !format.is_raster() {
		bail!(
			"Tile format conversion only supports raster formats (avif, jpg, png, webp), got '{}'",
			parts[0]
		);
	}

	let quality = if parts.len() >= 2 {
		let q: u8 = parts[1]
			.parse()
			.map_err(|_| anyhow::anyhow!("Invalid quality value '{}': must be 0-100", parts[1]))?;
		if q > 100 {
			bail!("Quality value {q} out of range: must be 0-100");
		}
		Some(q)
	} else {
		None
	};

	let effort = if parts.len() >= 3 {
		let e: u8 = parts[2]
			.parse()
			.map_err(|_| anyhow::anyhow!("Invalid effort value '{}': must be 0-100", parts[2]))?;
		if e > 100 {
			bail!("Effort value {e} out of range: must be 0-100");
		}
		Some(e)
	} else {
		None
	};

	Ok((format, quality, effort))
}

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// Input tile container (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(verbatim_doc_comment)]
	input_file: String,

	/// Output tile container path or SFTP URL.
	/// Supported formats: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory.
	/// SFTP URLs: sftp://[user[:pass]@]host[:port]/path (requires ssh2 feature)
	#[arg(verbatim_doc_comment)]
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

	/// swap rows and columns, e.g. z/x/y -> z/y/x
	#[arg(long, display_order = 3)]
	swap_xy: bool,

	/// flip input vertically
	#[arg(long, display_order = 3)]
	flip_y: bool,

	/// set the output tile format, e.g. "webp", "webp,80", "avif,90,50"
	#[arg(long, value_name = "format[,quality][,effort]", display_order = 3)]
	tile_format: Option<String>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	log::info!("convert from {:?} to {:?}", arguments.input_file, arguments.output_file);

	let reader = runtime.get_reader_from_str(&arguments.input_file).await?;

	let (bbox_pyramid, geo_bbox) = get_bbox_pyramid(arguments)?;

	let (tile_format, format_quality, format_effort) = if let Some(ref tf) = arguments.tile_format {
		let (fmt, q, s) = parse_tile_format(tf)?;
		(Some(fmt), q, s)
	} else {
		(None, None, None)
	};

	let parameters = TilesConverterParameters {
		bbox_pyramid,
		geo_bbox,
		flip_y: arguments.flip_y,
		swap_xy: arguments.swap_xy,
		tile_compression: arguments.compress,
		tile_format,
		format_quality,
		format_effort,
	};

	convert_tiles_container_to_str(reader, parameters, &arguments.output_file, runtime.clone()).await?;

	log::info!("finished converting tiles");

	Ok(())
}

#[context("Failed to get bounding box pyramid")]
fn get_bbox_pyramid(arguments: &Subcommand) -> Result<(Option<TilePyramid>, Option<GeoBBox>)> {
	if arguments.min_zoom.is_none() && arguments.max_zoom.is_none() && arguments.bbox.is_none() {
		return Ok((None, None));
	}

	let mut bbox_pyramid = TilePyramid::new_full();
	let mut geo_bbox = None;

	if let Some(level_min) = arguments.min_zoom {
		bbox_pyramid.set_level_min(level_min);
	}

	if let Some(level_max) = arguments.max_zoom {
		bbox_pyramid.set_level_max(level_max);
	}

	if let Some(bbox) = &arguments.bbox {
		log::trace!("parsing bbox argument: {bbox:?}");
		let values: Vec<f64> = bbox
			.split(&[' ', ',', ';'])
			.filter(|s| !s.is_empty())
			.map(|s| {
				s.parse::<f64>()
					.map_err(|_| anyhow::anyhow!("bbox value '{s}' is not a valid number"))
			})
			.collect::<Result<Vec<f64>>>()?;

		if values.len() != 4 {
			bail!("bbox must contain exactly 4 values, got {}: {bbox:?}", values.len());
		}

		let bbox = GeoBBox::try_from(values)?;
		bbox_pyramid.intersect_geo_bbox(&bbox)?;
		geo_bbox = Some(bbox);

		if let Some(b) = arguments.bbox_border {
			bbox_pyramid.buffer(b);
		}
	}

	Ok((Some(bbox_pyramid), geo_bbox))
}

#[cfg(test)]
mod tests {
	use super::parse_tile_format;
	use crate::tests::run_command;
	use anyhow::Result;
	use assert_fs::TempDir;
	use rstest::rstest;
	use versatiles_core::TileFormat;

	#[rstest]
	#[case("webp", TileFormat::WEBP, None, None)]
	#[case("webp,80", TileFormat::WEBP, Some(80), None)]
	#[case("avif,90,50", TileFormat::AVIF, Some(90), Some(50))]
	fn test_parse_tile_format(
		#[case] input: &str,
		#[case] expected_format: TileFormat,
		#[case] expected_quality: Option<u8>,
		#[case] expected_effort: Option<u8>,
	) {
		let (fmt, q, e) = parse_tile_format(input).unwrap();
		assert_eq!(fmt, expected_format);
		assert_eq!(q, expected_quality);
		assert_eq!(e, expected_effort);
	}

	#[test]
	fn test_parse_tile_format_rejects_non_raster() {
		assert!(parse_tile_format("mvt").is_err());
		assert!(parse_tile_format("json").is_err());
	}

	#[test]
	fn test_parse_tile_format_rejects_invalid() {
		assert!(parse_tile_format("webp,abc").is_err()); // invalid quality
		assert!(parse_tile_format("webp,80,abc").is_err()); // invalid effort
		assert!(parse_tile_format("unknown").is_err());
	}

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
