use std::{collections::BTreeMap, fs, time::Instant};

use anyhow::{Result, bail};
use versatiles_container::{
	DataLocation, DataSource, TilesConverterParameters, TilesRuntime, convert_tiles_container_to_str,
};
use versatiles_core::{GeoBBox, GeoCrop, TileCompression, TileFormat, TilePyramid};
use versatiles_derive::context;
use versatiles_pipeline::{check_pipeline, vpl::parse_vpl};

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

/// Parse repeated `-w key=value` arguments into the map the runtime carries.
///
/// A later occurrence of a key replaces an earlier one, matching how a shell
/// user expects a repeated flag to behave.
fn parse_writer_options(args: &[String]) -> Result<BTreeMap<String, String>> {
	let mut options = BTreeMap::new();
	for arg in args {
		let (key, value) = arg
			.split_once('=')
			.ok_or_else(|| anyhow::anyhow!("invalid writer option '{arg}': expected key=value"))?;
		let key = key.trim();
		if key.is_empty() {
			bail!("invalid writer option '{arg}': the key is empty");
		}
		options.insert(key.to_string(), value.trim().to_string());
	}
	Ok(options)
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
	/// SFTP URLs: sftp://[user[:pass]@]host[:port]/path (requires the sftp feature)
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

	/// set a format-specific option on the output writer, as key=value
	///
	/// Repeatable. Which options exist depends on the output format; an option
	/// the chosen writer does not accept is an error, not a no-op, and the
	/// message lists what that format does accept.
	/// Example: `--writer-option allow_unclustered=true` when writing PMTiles.
	#[arg(long, value_name = "key=value", display_order = 4, verbatim_doc_comment)]
	writer_option: Vec<String>,

	/// check the pipeline and exit without reading or writing any tiles
	///
	/// Reports mistakes in the pipeline itself — an unknown operation or
	/// parameter, a missing required parameter, a composite without sources.
	/// Nothing is opened, so a typo fails in the first second rather than after
	/// the inputs have been read.
	#[arg(long, display_order = 4, verbatim_doc_comment)]
	dry_run: bool,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	log::info!("convert from {:?} to {:?}", arguments.input_file, arguments.output_file);

	// Conversion must fail loudly on any silently-dropped tile: a truncated
	// output would be indistinguishable from a successful conversion.
	runtime.set_abort_on_error(true);
	runtime.set_writer_options(parse_writer_options(&arguments.writer_option)?);

	// Periodically log RSS so memory growth (e.g. toward an OOM kill, which the
	// process can't report itself) is visible in the conversion's own output.
	let _memory_heartbeat = super::memory::start();

	if arguments.dry_run {
		return dry_run(arguments);
	}

	log::trace!("convert: opening source '{}'", arguments.input_file);
	let open_start = Instant::now();
	let reader = runtime.reader_from_str(&arguments.input_file).await?;
	log::trace!("convert: source opened in {:.2}s", open_start.elapsed().as_secs_f32());

	let (tile_pyramid, geo_bbox) = tile_pyramid(arguments)?;

	let (tile_format, format_quality, format_effort) = if let Some(ref tf) = arguments.tile_format {
		let (fmt, q, s) = parse_tile_format(tf)?;
		(Some(fmt), q, s)
	} else {
		(None, None, None)
	};

	let parameters = TilesConverterParameters {
		tile_pyramid,
		geo_bbox,
		bbox_border: arguments.bbox_border,
		flip_y: arguments.flip_y,
		swap_xy: arguments.swap_xy,
		tile_compression: arguments.compress,
		tile_format,
		format_quality,
		format_effort,
	};

	log::trace!("convert: starting tile stream to '{}'", arguments.output_file);
	let stream_start = Instant::now();
	convert_tiles_container_to_str(reader, parameters, &arguments.output_file, runtime.clone()).await?;
	log::trace!(
		"convert: tile streaming complete in {:.2}s",
		stream_start.elapsed().as_secs_f32()
	);

	log::info!("finished converting tiles");

	Ok(())
}

#[context("Failed to get tile pyramid")]
fn tile_pyramid(arguments: &Subcommand) -> Result<(Option<TilePyramid>, Option<GeoBBox>)> {
	if arguments.min_zoom.is_none() && arguments.max_zoom.is_none() && arguments.bbox.is_none() {
		return Ok((None, None));
	}

	let mut tile_pyramid = TilePyramid::new_full();
	let mut geo_bbox = None;

	if let Some(level_min) = arguments.min_zoom {
		tile_pyramid.set_level_min(level_min);
	}

	if let Some(level_max) = arguments.max_zoom {
		tile_pyramid.set_level_max(level_max);
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
		// Same rule the `filter` operation applies, so `--bbox-border N` and
		// `filter bbox_border=N` cannot drift apart.
		GeoCrop::new(bbox, arguments.bbox_border.unwrap_or(0)).apply_to_pyramid(&mut tile_pyramid)?;
		geo_bbox = Some(bbox);
	}

	Ok((Some(tile_pyramid), geo_bbox))
}

/// Reports problems with the pipeline without opening anything.
///
/// Only a VPL source can be checked statically; for a plain container there is
/// nothing to validate short of opening it, which is exactly what `--dry-run`
/// promises not to do.
#[context("Checking '{}'", arguments.input_file)]
fn dry_run(arguments: &Subcommand) -> Result<()> {
	let source = DataSource::parse(&arguments.input_file)?;

	if source.optional_container_type() != Some("vpl") {
		log::info!(
			"'{}' is not a VPL pipeline, so there is nothing to check without opening it",
			arguments.input_file
		);
		return Ok(());
	}

	let vpl = match source.location() {
		DataLocation::Blob(blob) => blob.as_str().to_string(),
		DataLocation::Path(path) => fs::read_to_string(path)?,
		DataLocation::Url(url) => bail!("cannot check a pipeline hosted at '{url}' without fetching it"),
	};

	// `check_pipeline` needs no factory and no runtime, which is what makes the
	// "opens nothing" promise checkable rather than merely intended.
	let problems = check_pipeline(&parse_vpl(&vpl)?);

	if problems.is_empty() {
		log::info!("pipeline is valid");
		return Ok(());
	}

	for problem in &problems {
		let node = problem
			.path
			.iter()
			.map(std::string::ToString::to_string)
			.collect::<Vec<_>>()
			.join(".");
		log::error!("node {node}: {}", problem.message);
	}

	bail!("found {} problem(s) in the pipeline", problems.len())
}

#[cfg(test)]
mod tests {
	use super::parse_writer_options;

	#[test]
	fn parse_writer_options_splits_on_the_first_equals() {
		let opts = parse_writer_options(&["allow_unclustered=true".into(), "temp_dir=/a=b".into()]).unwrap();
		assert_eq!(opts.get("allow_unclustered").map(String::as_str), Some("true"));
		// A value may itself contain '=' — only the first one separates.
		assert_eq!(opts.get("temp_dir").map(String::as_str), Some("/a=b"));
	}

	#[test]
	fn parse_writer_options_trims_and_allows_empty_values() {
		let opts = parse_writer_options(&["  key  =  value  ".into(), "empty=".into()]).unwrap();
		assert_eq!(opts.get("key").map(String::as_str), Some("value"));
		assert_eq!(opts.get("empty").map(String::as_str), Some(""));
	}

	#[test]
	fn parse_writer_options_last_occurrence_wins() {
		let opts = parse_writer_options(&["k=1".into(), "k=2".into()]).unwrap();
		assert_eq!(opts.get("k").map(String::as_str), Some("2"));
	}

	#[test]
	fn parse_writer_options_rejects_malformed_input() {
		let err = parse_writer_options(&["nonsense".into()]).unwrap_err().to_string();
		assert!(err.contains("expected key=value"), "unexpected: {err}");

		let err = parse_writer_options(&["=value".into()]).unwrap_err().to_string();
		assert!(err.contains("the key is empty"), "unexpected: {err}");
	}
	use anyhow::Result;
	use assert_fs::TempDir;
	use rstest::rstest;
	use versatiles_core::TileFormat;

	use super::parse_tile_format;
	use crate::tests::run_command;

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
		let server = crate::test_http_server::TestHttpServer::shared();
		let temp_dir = TempDir::new()?;
		let temp_path = temp_dir.path().display();

		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=1",
			"--max-zoom=2",
			"--bbox=-180,-85,180,85",
			"--flip-y",
			&server.url("berlin.pmtiles"),
			&format!("{temp_path}/remote1.versatiles"),
		])?;
		Ok(())
	}

	#[test]
	fn test_remote2() -> Result<()> {
		let server = crate::test_http_server::TestHttpServer::shared();
		let temp_dir = TempDir::new()?;
		let temp_path = temp_dir.path().display();

		run_command(vec![
			"versatiles",
			"convert",
			"--min-zoom=12",
			"--bbox=13.38,52.46,13.43,52.49",
			"--flip-y",
			&server.url("berlin.pmtiles"),
			&format!("{temp_path}/remote2.versatiles"),
		])?;
		Ok(())
	}
}
