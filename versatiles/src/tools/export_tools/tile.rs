//! `versatiles export tile`: one tile out of a container, as a file of its own.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use versatiles_container::TilesRuntime;
use versatiles_core::{TileCompression, TileCoord, TileFormat, TileType};

use super::{geojson, output};

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	about = "Export one tile as a file",
	long_about = "\
Export one tile as a file.

The tile is written as the container holds it, decompressed unless --raw is
given. Address it as z/x/y, in XYZ order as in tile URLs, or as the tile at a
point with --at and --zoom. A tile that the container does not have is an
error.

A vector tile can be decoded into GeoJSON instead, with --as geojson or an
output file ending in .geojson. Coordinates are in degrees, and each feature
gains the properties $layer and $tile. --layer keeps only the layers named.

EXAMPLES

  # The tile 14/8801/5373, as a decompressed vector tile.
  versatiles export tile osm.versatiles 14/8801/5373 -o tile.pbf

  # The satellite tile at zoom 17 that contains a point.
  versatiles export tile satellite.versatiles --at 13.40,52.52 --zoom 17 -o berlin.webp

  # The tile a pipeline produces, without writing a container.
  versatiles export tile pipeline.vpl 16/35210/21490 > tile.webp

  # The buildings of a vector tile, as GeoJSON on the terminal.
  versatiles export tile osm.versatiles 14/8801/5373 --as geojson --layer buildings"
)]
pub struct Tile {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// The tile, as z/x/y.
	#[arg(value_name = "Z/X/Y", required_unless_present = "at", conflicts_with_all = ["at", "zoom"])]
	coord: Option<String>,

	/// Export the tile that contains this point, given as lon,lat. Needs --zoom.
	#[arg(long, value_name = "lon,lat", requires = "zoom", allow_hyphen_values = true)]
	at: Option<String>,

	/// The zoom level of the tile --at looks for.
	#[arg(long, value_name = "int", requires = "at")]
	zoom: Option<u8>,

	/// File to write the tile to. Its extension has to fit the tile, such as
	/// `.pbf` for a vector tile or `.webp.gz` with --raw, or be `.geojson` to
	/// decode a vector tile; a name without a known extension is accepted as
	/// it is. Defaults to standard output, which must not be a terminal unless
	/// the output is GeoJSON.
	#[arg(short, long, value_name = "FILE", verbatim_doc_comment)]
	output: Option<PathBuf>,

	/// Write the tile as stored, without decompressing it.
	#[arg(long)]
	raw: bool,

	/// Decode the tile into this format rather than writing it as it is.
	#[arg(long = "as", value_name = "FORMAT", value_enum)]
	format: Option<Decoded>,

	/// With GeoJSON, keep only the features of this layer. Repeat for several layers.
	#[arg(long, value_name = "NAME")]
	layer: Vec<String>,
}

/// What `--as` decodes a tile into.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decoded {
	/// The features of a vector tile, in degrees.
	Geojson,
}

pub async fn run(args: &Tile, runtime: &TilesRuntime) -> Result<()> {
	let coord = address(args)?;
	if wants_geojson(args) {
		return run_geojson(args, coord, runtime).await;
	}
	ensure!(
		args.layer.is_empty(),
		"--layer selects the layers of GeoJSON output: add --as geojson, or name the output file .geojson"
	);
	if args.output.is_none() {
		output::ensure_stdout_is_not_a_terminal()?;
	}

	let reader = runtime
		.reader_from_str(&args.input)
		.await
		.with_context(|| format!("opening {}", args.input))?;
	let format = *reader.metadata().tile_format();
	let stored = *reader.metadata().tile_compression();
	let compression = if args.raw {
		stored
	} else {
		TileCompression::Uncompressed
	};
	if let Some(path) = &args.output {
		output::check_extension(path, format, compression, stored)?;
	}

	let Some(tile) = reader.tile(&coord).await? else {
		bail!("{} has no tile {}", args.input, format_coord(&coord));
	};
	let blob = tile.into_blob(&compression)?;
	output::write(blob.as_slice(), args.output.as_deref())?;
	log::info!("exported tile {} ({} bytes)", format_coord(&coord), blob.len());
	Ok(())
}

/// Whether the tile is decoded into GeoJSON: asked for with `--as`, or by the
/// extension of the output file.
fn wants_geojson(args: &Tile) -> bool {
	args.format == Some(Decoded::Geojson) || args.output.as_deref().is_some_and(names_geojson)
}

fn names_geojson(path: &Path) -> bool {
	path
		.extension()
		.is_some_and(|extension| extension.eq_ignore_ascii_case("geojson"))
}

/// Writes the features of a vector tile as GeoJSON.
async fn run_geojson(args: &Tile, coord: TileCoord, runtime: &TilesRuntime) -> Result<()> {
	ensure!(
		!args.raw,
		"--raw writes the tile as it is stored, and GeoJSON is decoded from it: leave out one of the two"
	);
	if let Some(path) = &args.output {
		output::check_extension(
			path,
			TileFormat::GEOJSON,
			TileCompression::Uncompressed,
			TileCompression::Uncompressed,
		)?;
	}

	let reader = runtime
		.reader_from_str(&args.input)
		.await
		.with_context(|| format!("opening {}", args.input))?;
	let format = *reader.metadata().tile_format();
	ensure!(
		format.to_type() == TileType::Vector,
		"GeoJSON is decoded from vector tiles, and {} holds {} tiles",
		args.input,
		format.as_str()
	);
	let Some(tile) = reader.tile(&coord).await? else {
		bail!("{} has no tile {}", args.input, format_coord(&coord));
	};

	let selection = geojson::Selection {
		layers: args.layer.clone(),
		clip: None,
	};
	let mut writer = geojson::Writer::new(output::create(args.output.as_deref())?)?;
	writer.write_tile(&coord, tile, &selection)?;
	let features = writer.finish(&selection)?;
	log::info!("exported {features} features of tile {}", format_coord(&coord));
	Ok(())
}

/// The tile the arguments name.
fn address(args: &Tile) -> Result<TileCoord> {
	match (&args.coord, &args.at, args.zoom) {
		(Some(text), None, None) => parse_coord(text),
		(None, Some(at), Some(zoom)) => {
			let (lon, lat) = parse_lon_lat(at)?;
			TileCoord::from_geo(lon, lat, zoom)
		}
		_ => bail!("give the tile as z/x/y, or as a point with --at and --zoom"),
	}
}

/// `z/x/y`, as in tile URLs.
fn parse_coord(text: &str) -> Result<TileCoord> {
	let parts: Vec<&str> = text.split('/').collect();
	ensure!(
		parts.len() == 3,
		"a tile is given as z/x/y, such as 14/8801/5373, not as '{text}'"
	);
	let level: u8 = parts[0]
		.trim()
		.parse()
		.map_err(|_| anyhow!("'{}' in '{text}' is not a zoom level", parts[0]))?;
	let x: u32 = parts[1]
		.trim()
		.parse()
		.map_err(|_| anyhow!("'{}' in '{text}' is not a column", parts[1]))?;
	let y: u32 = parts[2]
		.trim()
		.parse()
		.map_err(|_| anyhow!("'{}' in '{text}' is not a row", parts[2]))?;
	TileCoord::new(level, x, y)
}

/// `lon,lat`, in degrees.
fn parse_lon_lat(text: &str) -> Result<(f64, f64)> {
	let (lon, lat) = text
		.split_once(',')
		.ok_or_else(|| anyhow!("a point is given as lon,lat, such as 13.40,52.52, not as '{text}'"))?;
	let parse = |value: &str, name: &str| -> Result<f64> {
		let number: f64 = value
			.trim()
			.parse()
			.map_err(|_| anyhow!("'{}' in '{text}' is not a {name}", value.trim()))?;
		ensure!(number.is_finite(), "'{}' in '{text}' is not a {name}", value.trim());
		Ok(number)
	};
	Ok((parse(lon, "longitude")?, parse(lat, "latitude")?))
}

fn format_coord(coord: &TileCoord) -> String {
	format!("{}/{}/{}", coord.level, coord.x, coord.y)
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("14/8801/5373", (14, 8801, 5373))]
	#[case(" 0 / 0 / 0 ", (0, 0, 0))]
	fn a_tile_is_given_as_z_x_y(#[case] text: &str, #[case] (level, x, y): (u8, u32, u32)) -> Result<()> {
		assert_eq!(parse_coord(text)?, TileCoord::new(level, x, y)?);
		Ok(())
	}

	#[rstest]
	#[case("14/8801", "z/x/y")]
	#[case("14/8801/5373/1", "z/x/y")]
	#[case("a/1/1", "not a zoom level")]
	#[case("1/-1/1", "not a column")]
	#[case("1/1/b", "not a row")]
	#[case("1/2/0", "out of bounds")]
	fn a_bad_tile_is_refused_with_a_reason(#[case] text: &str, #[case] needle: &str) {
		let error = format!("{:#}", parse_coord(text).unwrap_err());
		assert!(error.contains(needle), "{text}: {error}");
	}

	#[test]
	fn a_point_is_given_as_lon_lat() -> Result<()> {
		assert_eq!(parse_lon_lat("13.40,52.52")?, (13.40, 52.52));
		assert_eq!(parse_lon_lat("-70.5, -33.4")?, (-70.5, -33.4));
		for (text, needle) in [
			("13.40", "lon,lat"),
			("x,52", "not a longitude"),
			("13,y", "not a latitude"),
			("NaN,52", "not a longitude"),
		] {
			let error = parse_lon_lat(text).unwrap_err().to_string();
			assert!(error.contains(needle), "{text}: {error}");
		}
		Ok(())
	}

	fn args(coord: Option<&str>, at: Option<&str>, zoom: Option<u8>) -> Tile {
		Tile {
			input: String::new(),
			coord: coord.map(String::from),
			at: at.map(String::from),
			zoom,
			output: None,
			raw: false,
			format: None,
			layer: vec![],
		}
	}

	#[test]
	fn a_point_names_the_tile_it_lies_in() -> Result<()> {
		assert_eq!(
			address(&args(None, Some("13.40,52.52"), Some(14)))?,
			TileCoord::new(14, 8801, 5373)?
		);
		assert_eq!(
			address(&args(Some("14/8801/5373"), None, None))?,
			TileCoord::new(14, 8801, 5373)?
		);
		let error = address(&args(None, Some("200,0"), Some(1))).unwrap_err();
		assert!(format!("{error:#}").contains("must be <= 180"), "{error:#}");
		Ok(())
	}
}
