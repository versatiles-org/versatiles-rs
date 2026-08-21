//! Converts tile data between formats, compressions, and coordinate conventions.
//!
//! This module provides:
//! - [`TilesConverterParameters`]: declarative knobs (bbox filter, compression override, `flip_y`, `swap_xy`)
//! - [`TilesConvertReader`]: an adapter that applies those conversions while reading
//! - [`convert_tiles_container`]: a convenience function to convert and write to a target path using a [`ContainerRegistry`]
//!
//! ## Coordinate transforms
//! - `flip_y`: inverts Y within the zoom level (useful to switch between TMS and XYZ-like schemes)
//! - `swap_xy`: swaps X and Y (occasionally useful for sources with unconventional axis ordering)
//!
//! ## Example
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!
//!     // Create runtime with default settings
//!     let runtime = TilesRuntime::default();
//!
//!     // Open the source
//!     let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
//!
//!     // Limit to a tile pyramid and keep source compression;
//!     // you could also set `tile_compression: Some(TileCompression::Brotli)` to re-encode.
//!     let converter_params = TilesConverterParameters {
//!         tile_pyramid: Some(TilePyramid::new_full_up_to(8)),
//!         ..Default::default()
//!     };
//!
//!     // Convert and write
//!     let path_versatiles = std::env::temp_dir().join("temp2.versatiles");
//!     convert_tiles_container(reader, converter_params, &path_versatiles.as_path(), runtime).await?;
//!
//!     println!("Wrote {:?}", path_versatiles);
//!     Ok(())
//! }
//! ```

use std::{path::Path, sync::Arc};

use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use versatiles_core::{
	GeoBBox, GeoCrop, TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileRemap, TileStream,
};
use versatiles_derive::context;

use crate::{RemappedTileSource, SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, TilesRuntime};

/// Parameters that control how tiles are transformed during reading/conversion.
///
/// These options affect coordinate handling, the subset of tiles traversed, and
/// whether tile payloads are re-encoded with a different compression.
///
/// Use with [`TilesConvertReader::new_from_reader`] or the helper
/// [`convert_tiles_container`].
#[derive(Debug)]
pub struct TilesConverterParameters {
	/// Optional spatial/zoom restriction. When set, only tiles inside the given
	/// [`TilePyramid`] are read/streamed. Existing bounds are intersected with this.
	pub tile_pyramid: Option<TilePyramid>,
	/// Optional geographic bounding box filter. When set, existing tilejson bounds are
	/// intersected with this bbox rather than being recalculated from the pyramid.
	pub geo_bbox: Option<GeoBBox>,
	/// Border ring (in tiles) added around `geo_bbox` when building `tile_pyramid`.
	/// When `> 0`, the advertised bounds are extended beyond `geo_bbox` to include
	/// the border tiles (which lie outside the crop), so a tile client requests
	/// them. `None`/`0` keeps the precise crop bounds.
	pub bbox_border: Option<u32>,
	/// Optional compression override. When set, tile payloads are re-encoded to this
	/// [`TileCompression`] (e.g., Gzip → Brotli). If `None`, the source compression is kept.
	pub tile_compression: Option<TileCompression>,
	/// Optional tile format override. When set, tiles are re-encoded to this format
	/// (e.g., PNG → WEBP). Only raster formats (AVIF, JPG, PNG, WEBP) are supported.
	pub tile_format: Option<TileFormat>,
	/// Optional quality parameter (0–100) for format conversion.
	pub format_quality: Option<u8>,
	/// Optional effort parameter (0–100) for format conversion.
	pub format_effort: Option<u8>,
	/// If `true`, flip the Y coordinate within the zoom level (TMS ↔ XYZ-like).
	pub flip_y: bool,
	/// If `true`, swap X and Y coordinates.
	pub swap_xy: bool,
}

impl Default for TilesConverterParameters {
	/// Returns parameters that perform no geometric change and keep source compression.
	fn default() -> Self {
		TilesConverterParameters {
			tile_pyramid: None,
			geo_bbox: None,
			bbox_border: None,
			tile_compression: None,
			tile_format: None,
			format_quality: None,
			format_effort: None,
			flip_y: false,
			swap_xy: false,
		}
	}
}

/// Converts tiles from the given reader and writes them to `path` using the provided runtime.
///
/// The conversion is applied by wrapping `reader` in a [`TilesConvertReader`] configured by `cp`.
///
/// ### Arguments
/// - `reader`: Source container reader.
/// - `cp`: Conversion parameters (bbox filter, compression override, `flip_y`, `swap_xy`).
/// - `path`: Output path; the format is inferred from its extension (or directory).
/// - `runtime`: Runtime configuration providing registry, cache, and event system.
///
/// ### Errors
/// Returns an error if reading tiles fails, if writing to the destination fails,
/// or if no suitable writer is registered for the output path.
///
/// ### Example
/// See the module-level example.
/// Default ceiling on how many tiles one conversion may write.
///
/// A writer's block index grows with the pyramid and is never freed, so a deep
/// enough pyramid exhausts memory long before it finishes — with no error, just
/// steadily climbing RSS. A source advertising levels 0-30 describes roughly
/// 1.5 x 10^18 tiles, which is not a slow job but an impossible one.
///
/// The value clears real work by a wide margin: a whole-planet pyramid is about
/// 3.6 x 10^8 tiles at level 14 and 1.4 x 10^9 at level 15. Anything in between
/// is far likelier to be a missing `--max-zoom` than an intention.
pub const DEFAULT_TILE_COUNT_LIMIT: u64 = 10_000_000_000;

/// Environment variable overriding [`DEFAULT_TILE_COUNT_LIMIT`]; `0` disables
/// the check.
pub const TILE_COUNT_LIMIT_ENV: &str = "VERSATILES_MAX_TILES";

/// The tile-count limit in force, from [`TILE_COUNT_LIMIT_ENV`] or
/// [`DEFAULT_TILE_COUNT_LIMIT`]. `0` maps to `u64::MAX`, i.e. no limit.
///
/// A value that does not parse is ignored rather than fatal: a typo in an
/// environment variable should not stop a conversion that would otherwise be
/// fine, and the limit it falls back to is the safe one.
#[must_use]
pub fn resolve_tile_count_limit() -> u64 {
	let Ok(value) = std::env::var(TILE_COUNT_LIMIT_ENV) else {
		return DEFAULT_TILE_COUNT_LIMIT;
	};
	match value.trim().parse::<u64>() {
		Ok(0) => u64::MAX,
		Ok(limit) => limit,
		Err(_) => {
			log::warn!(
				"ignoring {TILE_COUNT_LIMIT_ENV}={value:?}: not a number; using the default limit of {DEFAULT_TILE_COUNT_LIMIT}"
			);
			DEFAULT_TILE_COUNT_LIMIT
		}
	}
}

/// Refuses a conversion whose pyramid is too large to be possible.
///
/// Counting is pure bounds arithmetic on the pyramid, so this costs nothing,
/// and it runs before the output is opened — nothing is created on disk when it
/// fires.
#[context("Checking the size of the conversion")]
fn check_tile_count(pyramid: &TilePyramid, limit: u64) -> Result<()> {
	let count = pyramid.count_tiles();
	if count <= limit {
		return Ok(());
	}

	let levels = match (pyramid.level_min(), pyramid.level_max()) {
		(Some(min), Some(max)) => format!("levels {min}-{max}"),
		_ => "an empty level range".to_string(),
	};

	// Only explain *why* a limit exists when the user did not pick it. Someone
	// who set VERSATILES_MAX_TILES knows; telling them their pyramid is
	// suspicious would be wrong as well as patronising.
	let hint = if limit == DEFAULT_TILE_COUNT_LIMIT {
		" The source advertises a pyramid this large, which is rarely intended."
	} else {
		""
	};

	bail!(
		"refusing to convert {} tiles ({levels}): more than the limit of {}.{hint} \
		 Restrict the job with --max-zoom and/or --bbox, or raise the ceiling with \
		 {TILE_COUNT_LIMIT_ENV} (0 disables the check).",
		group_digits(count),
		group_digits(limit)
	)
}

/// Formats a count with thousands separators.
///
/// These numbers span nine orders of magnitude, and the point of the message is
/// that the reader can tell 10 billion from 1.5 quintillion at a glance.
fn group_digits(value: u64) -> String {
	let digits = value.to_string();
	let mut out = String::with_capacity(digits.len() * 4 / 3);
	for (i, c) in digits.chars().enumerate() {
		if i > 0 && (digits.len() - i).is_multiple_of(3) {
			out.push('_');
		}
		out.push(c);
	}
	out
}

#[context("Converting tiles from reader to file")]
pub async fn convert_tiles_container(
	reader: SharedTileSource,
	cp: TilesConverterParameters,
	path: &Path,
	runtime: TilesRuntime,
) -> Result<()> {
	runtime.events().step("Starting conversion".to_string());

	let converter = TilesConvertReader::new_from_reader(reader, cp).await?;
	let pyramid = converter.tile_pyramid().await?;
	check_tile_count(&pyramid, resolve_tile_count_limit())?;
	runtime.write_to_path(converter.into_shared(), path).await?;

	if runtime.had_errors() {
		bail!(
			"conversion completed with {} read error(s) — output may be incomplete",
			runtime.error_count()
		);
	}

	runtime.events().step("Conversion complete".to_string());
	Ok(())
}

/// Converts tiles from the given reader and writes them to a string destination.
///
/// The destination can be a file path or an SFTP URL (`sftp://...`).
#[context("Converting tiles from reader to destination")]
pub async fn convert_tiles_container_to_str(
	reader: SharedTileSource,
	cp: TilesConverterParameters,
	destination: &str,
	runtime: TilesRuntime,
) -> Result<()> {
	runtime.events().step("Starting conversion".to_string());

	let converter = TilesConvertReader::new_from_reader(reader, cp).await?;
	let pyramid = converter.tile_pyramid().await?;
	check_tile_count(&pyramid, resolve_tile_count_limit())?;
	runtime.write_to_str(converter.into_shared(), destination).await?;

	if runtime.had_errors() {
		bail!(
			"conversion completed with {} read error(s) — output may be incomplete",
			runtime.error_count()
		);
	}

	runtime.events().step("Conversion complete".to_string());
	Ok(())
}

/// Reader adapter that applies coordinate transforms, bbox filtering, and optional
/// compression changes on-the-fly.
///
/// This type implements [`TileSource`], so it can be used anywhere a normal
/// reader is expected. Use [`TilesConvertReader::new_from_reader`] to wrap an
/// existing reader.
#[derive(Debug)]
pub struct TilesConvertReader {
	reader: SharedTileSource,
	converter_parameters: TilesConverterParameters,
	reader_metadata: TileSourceMetadata,
	tilejson: TileJSON,
	tile_pyramid: Arc<TilePyramid>,
}

impl TilesConvertReader {
	/// Wraps an existing reader so that reads are transformed according to `cp`.
	///
	/// Updates the internal [`TileSourceMetadata`] and [`TileJSON`] to reflect
	/// the chosen bbox, axis transforms, and compression.
	///
	/// ### Errors
	/// Propagates errors from querying/deriving parameters or updating metadata.
	#[context("Creating converter reader from existing reader")]
	pub async fn new_from_reader(reader: SharedTileSource, cp: TilesConverterParameters) -> Result<TilesConvertReader> {
		// Coordinate remapping is delegated in full: the wrapper maps requests
		// backwards and results forwards for every access path, which is what
		// this type used to do by hand — inconsistently (issue #230).
		let remap = TileRemap::new(false, cp.flip_y, cp.swap_xy);
		let reader: SharedTileSource = if remap.is_identity() {
			reader
		} else {
			Arc::new(Box::new(RemappedTileSource::new(reader, remap).await?) as Box<dyn TileSource>)
		};

		let mut tile_pyramid = reader.tile_pyramid().await?.as_ref().clone();

		if let Some(filter_pyramid) = &cp.tile_pyramid {
			tile_pyramid.intersect_pyramid(filter_pyramid);
		}

		let mut new_rp: TileSourceMetadata = reader.metadata().clone();

		if let Some(tile_format) = cp.tile_format {
			// Checked here rather than per tile: `Tile::change_format` cannot turn a
			// vector tile into a raster one (or back), and finding that out inside a
			// spawned stream task buries the reason. One comparison, before the
			// pyramid is walked, so the caller gets the error immediately (issue #244).
			let source_format = *reader.metadata().tile_format();
			ensure!(
				tile_format.to_type() == source_format.to_type(),
				"cannot convert to '{tile_format}': it is a {} format, but the source contains {} tiles ('{source_format}')",
				tile_format.to_type().as_str(),
				source_format.to_type().as_str()
			);
			new_rp.set_tile_format(tile_format);
		}

		if let Some(tile_compression) = cp.tile_compression {
			new_rp.set_tile_compression(tile_compression);
		}

		// Expose the (possibly transformed/filtered) pyramid through the metadata so
		// downstream consumers (e.g. `update_tilejson`, writers) can derive bounds,
		// zoom range, and center from it.
		new_rp.set_tile_pyramid(tile_pyramid.clone());

		let mut tilejson = reader.tilejson().clone();

		if let Some(ref geo_bbox) = cp.geo_bbox {
			let crop = GeoCrop::new(*geo_bbox, cp.bbox_border.unwrap_or(0));
			tilejson.bounds = Some(crop.bounds(tilejson.bounds, &tile_pyramid));
			tilejson.center = None;
		}
		new_rp.update_tilejson(&mut tilejson);

		Ok(TilesConvertReader {
			reader,
			converter_parameters: cp,
			reader_metadata: new_rp,
			tilejson,
			tile_pyramid: Arc::new(tile_pyramid),
		})
	}
}

#[async_trait]
impl TileSource for TilesConvertReader {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("TilesConvertReader", self.reader.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.reader_metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		let tile = self.reader.tile(coord).await?;

		let Some(mut tile) = tile else { return Ok(None) };

		if let Some(tile_format) = self.converter_parameters.tile_format {
			tile.change_format(
				tile_format,
				self.converter_parameters.format_quality,
				self.converter_parameters.format_effort,
			)?;
		}

		if let Some(compression) = self.converter_parameters.tile_compression {
			tile.change_compression(&compression)?;
		}

		Ok(Some(tile))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.reader.tile_coord_stream(bbox).await
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("converter::tile_stream {bbox:?}");

		let mut stream = self.reader.tile_stream(bbox).await?;

		if let Some(tile_format) = self.converter_parameters.tile_format {
			let quality = self.converter_parameters.format_quality;
			let effort = self.converter_parameters.format_effort;
			stream = stream
				.map_parallel_try(move |_coord, mut tile| {
					tile.change_format(tile_format, quality, effort)?;
					Ok(tile)
				})
				.unwrap_results();
		}

		if let Some(tile_compression) = self.converter_parameters.tile_compression {
			stream = stream
				.map_parallel_try(move |_coord, mut tile| {
					tile.change_compression(&tile_compression)?;
					Ok(tile)
				})
				.unwrap_results();
		}

		Ok(stream)
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		Ok(self.tile_pyramid.clone())
	}
}

/// Integration tests verifying bbox intersection, coordinate transforms, traversal order,
/// and compression override behavior.
#[cfg(test)]
mod tests {
	use assert_fs::NamedTempFile;
	use rstest::rstest;
	use versatiles_core::{
		GeoBBox, TileBBox,
		TileCompression::*,
		TileFormat::{self, *},
		TilePyramid,
	};

	use super::*;
	use crate::{MockReader, Traversal, VersaTilesReader};

	fn new_pyramid(b: [u32; 4]) -> TilePyramid {
		TilePyramid::from([TileBBox::from_min_and_max(3, b[0], b[1], b[2], b[3]).unwrap()].as_slice())
	}

	fn get_mock_reader(tf: TileFormat, tc: TileCompression) -> SharedTileSource {
		let tile_pyramid = TilePyramid::new_full_up_to(4);
		let reader_metadata = TileSourceMetadata::new(tf, tc, Traversal::ANY, None);
		MockReader::new_mock(tile_pyramid, reader_metadata)
			.unwrap()
			.into_shared()
	}

	#[rstest]
	#[case(false, false, [2, 3, 4, 5], "23 33 43 24 34 25 35 44 45")]
	#[case(false, true, [2, 3, 5, 4], "32 33 34 35 42 43 44 45")]
	#[case(true, false, [2, 3, 4, 6], "24 34 44 23 33 22 32 21 31 43 42 41")]
	#[case(true, true, [2, 3, 6, 4], "35 34 33 32 31 45 44 43 42 41")]
	#[tokio::test]
	async fn bbox_and_tile_order(
		#[case] flip_y: bool,
		#[case] swap_xy: bool,
		#[case] bbox_out: [u32; 4],
		#[case] tile_list: &str,
	) -> Result<()> {
		let pyramid_in = new_pyramid([0, 1, 4, 5]);
		let pyramid_convert = new_pyramid([2, 3, 7, 7]);
		let pyramid_out = new_pyramid(bbox_out);

		let reader_metadata = TileSourceMetadata::new(JSON, Uncompressed, Traversal::ANY, None);
		let reader = MockReader::new_mock(pyramid_in, reader_metadata)?.into_shared();

		let temp_file = NamedTempFile::new("test.versatiles")?;
		let runtime = TilesRuntime::default();

		let cp = TilesConverterParameters {
			tile_pyramid: Some(pyramid_convert),
			geo_bbox: None,
			flip_y,
			swap_xy,
			tile_compression: None,
			..Default::default()
		};
		convert_tiles_container(reader, cp, &temp_file, runtime.clone()).await?;

		let reader_out = VersaTilesReader::open(&temp_file, runtime).await?;
		let parameters_out = reader_out.metadata();
		let tile_compression_out = parameters_out.tile_compression();
		assert_eq!(reader_out.tile_pyramid().await?.as_ref(), &pyramid_out);

		let bbox = pyramid_out.level_ref(3).to_bbox();
		let mut tiles: Vec<String> = Vec::new();
		for coord in bbox.iter_coords_zorder() {
			let mut text = reader_out
				.tile(&coord)
				.await?
				.unwrap()
				.into_blob(tile_compression_out)?
				.to_string();
			text = text
				.replace("{\"z\":3,\"x\":", "")
				.replace(",\"y\":", "")
				.replace('}', "");
			tiles.push(text);
		}
		let tiles = tiles.join(" ");
		assert_eq!(tiles, tile_list);

		Ok(())
	}

	#[test]
	fn test_tiles_converter_parameters_new() {
		let cp = TilesConverterParameters {
			tile_pyramid: Some(TilePyramid::new_full_up_to(1)),
			geo_bbox: None,
			flip_y: true,
			swap_xy: true,
			tile_compression: None,
			..Default::default()
		};

		assert!(cp.tile_pyramid.is_some());
		assert!(cp.flip_y);
		assert!(cp.swap_xy);
	}

	#[test]
	fn test_tiles_converter_parameters_default() {
		let cp = TilesConverterParameters::default();

		assert_eq!(cp.tile_pyramid, None);
		assert!(!cp.flip_y);
		assert!(!cp.swap_xy);
	}

	#[tokio::test]
	async fn test_tiles_convert_reader_new_from_reader() -> Result<()> {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters::default();

		let tcr = TilesConvertReader::new_from_reader(reader, cp).await?;

		assert_eq!(tcr.reader.source_type().to_string(), "container 'dummy' ('dummy')");
		assert_eq!(tcr.source_type().to_string(), "processor 'TilesConvertReader'");
		Ok(())
	}

	#[tokio::test]
	async fn test_tile() -> Result<()> {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters::default();
		let tcr = TilesConvertReader::new_from_reader(reader, cp).await?;

		let coord = TileCoord::new(0, 0, 0)?;
		let data = tcr.tile(&coord).await?;
		assert!(data.is_some());

		Ok(())
	}

	#[tokio::test]
	async fn test_flip_y_and_swap_xy() -> Result<()> {
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters {
			flip_y: true,
			swap_xy: true,
			..Default::default()
		};
		let tcr = TilesConvertReader::new_from_reader(reader, cp).await?;

		let mut coord = TileCoord::new(4, 5, 6)?;
		let data = tcr.tile(&coord).await?;
		assert!(data.is_some());

		coord.flip_y();
		coord.swap_xy();
		let data_flipped = tcr.tile(&coord).await?;
		assert_eq!(data, data_flipped);

		Ok(())
	}

	#[tokio::test]
	async fn test_geo_bbox_intersects_existing_tilejson_bounds() -> Result<()> {
		// Source has bounds covering a wide area
		let source_bounds = GeoBBox::new(10.0, 50.0, 15.0, 55.0)?;
		let tile_pyramid = TilePyramid::new_full_up_to(4);
		let reader_metadata = TileSourceMetadata::new(MVT, Uncompressed, Traversal::ANY, None);
		let mut reader = MockReader::new_mock(tile_pyramid, reader_metadata)?;
		reader.tilejson_mut().bounds = Some(source_bounds);
		let reader = reader.into_shared();

		// User specifies a bbox that partially overlaps, without a border, so the
		// precise crop bounds are kept (intersected with the source bounds).
		let filter_bbox = GeoBBox::new(12.0, 52.0, 20.0, 60.0)?;
		let mut filter_pyramid = TilePyramid::new_full();
		filter_pyramid.intersect_geo_bbox(&filter_bbox)?;

		let cp = TilesConverterParameters {
			tile_pyramid: Some(filter_pyramid),
			geo_bbox: Some(filter_bbox),
			..Default::default()
		};
		let tcr = TilesConvertReader::new_from_reader(reader, cp).await?;
		let bounds = tcr.tilejson().bounds.ok_or_else(|| anyhow::anyhow!("missing bounds"))?;

		// Bounds should be the intersection: [12.0, 52.0, 15.0, 55.0]
		assert_eq!(bounds.as_tuple(), (12.0, 52.0, 15.0, 55.0));
		Ok(())
	}

	#[tokio::test]
	async fn test_bbox_border_extends_bounds_to_written_tiles() -> Result<()> {
		// With a border, bounds must grow beyond the crop to include the border
		// tiles that were written outside it.
		let tile_pyramid = TilePyramid::new_full_up_to(12);
		let reader_metadata = TileSourceMetadata::new(MVT, Uncompressed, Traversal::ANY, None);
		let reader = MockReader::new_mock(tile_pyramid, reader_metadata)?.into_shared();

		let filter_bbox = GeoBBox::new(13.35, 52.48, 13.40, 52.52)?;
		let mut filter_pyramid = TilePyramid::new_full();
		filter_pyramid.intersect_geo_bbox(&filter_bbox)?;
		filter_pyramid.set_level_min(10);
		filter_pyramid.buffer(3);

		let cp = TilesConverterParameters {
			tile_pyramid: Some(filter_pyramid),
			geo_bbox: Some(filter_bbox),
			bbox_border: Some(3),
			..Default::default()
		};
		let tcr = TilesConvertReader::new_from_reader(reader, cp).await?;
		let b = tcr.tilejson().bounds.ok_or_else(|| anyhow::anyhow!("missing bounds"))?;

		// Every side is pushed outside the requested crop (a 3-tile ring at z10 is
		// ~1° wide), but the box stays regional — nowhere near the whole world.
		assert!(b.x_min < 13.35 && b.x_min > 11.0, "west: {b:?}");
		assert!(b.y_min < 52.48 && b.y_min > 50.0, "south: {b:?}");
		assert!(b.x_max > 13.40 && b.x_max < 16.0, "east: {b:?}");
		assert!(b.y_max > 52.52 && b.y_max < 55.0, "north: {b:?}");
		Ok(())
	}

	#[tokio::test]
	async fn test_geo_bbox_without_existing_bounds_uses_pyramid() -> Result<()> {
		// Source has NO explicit bounds in tilejson
		let tile_pyramid = TilePyramid::new_full_up_to(4);
		let reader_metadata = TileSourceMetadata::new(MVT, Uncompressed, Traversal::ANY, None);
		let reader = MockReader::new_mock(tile_pyramid, reader_metadata)?.into_shared();

		let filter_bbox = GeoBBox::new(12.0, 52.0, 14.0, 54.0)?;
		let mut filter_pyramid = TilePyramid::new_full();
		filter_pyramid.intersect_geo_bbox(&filter_bbox)?;

		let cp = TilesConverterParameters {
			tile_pyramid: Some(filter_pyramid),
			geo_bbox: Some(filter_bbox),
			..Default::default()
		};
		let tcr = TilesConvertReader::new_from_reader(reader, cp).await?;

		// Bounds should be set from the pyramid (since no existing bounds to intersect)
		assert!(tcr.tilejson().bounds.is_some());
		Ok(())
	}

	#[tokio::test]
	async fn test_format_conversion() -> Result<()> {
		let reader = get_mock_reader(PNG, Uncompressed);
		let cp = TilesConverterParameters {
			tile_format: Some(WEBP),
			format_quality: Some(80),
			..Default::default()
		};
		let tcr = TilesConvertReader::new_from_reader(reader, cp).await?;

		assert_eq!(tcr.metadata().tile_format(), &WEBP);
		assert_eq!(tcr.metadata().tile_compression(), &Uncompressed);

		let coord = TileCoord::new(0, 0, 0)?;
		let tile = tcr.tile(&coord).await?.unwrap();
		assert_eq!(tile.format(), WEBP);

		Ok(())
	}

	#[tokio::test]
	async fn test_format_conversion_rejects_mismatched_tile_type() -> Result<()> {
		// Issue #244: `--tile-format webp` against a vector container used to reach
		// an assertion inside a spawned stream task. It has to fail here, while the
		// reader is being built, before a single tile is read.
		let reader = get_mock_reader(MVT, Uncompressed);
		let cp = TilesConverterParameters {
			tile_format: Some(WEBP),
			..Default::default()
		};
		let err = TilesConvertReader::new_from_reader(reader, cp).await.unwrap_err();
		assert!(
			format!("{err:#}")
				.contains("cannot convert to 'webp': it is a raster format, but the source contains vector tiles ('mvt')"),
			"unexpected message: {err:#}"
		);
		Ok(())
	}

	#[tokio::test]
	async fn test_format_conversion_with_compression() -> Result<()> {
		let reader = get_mock_reader(PNG, Uncompressed);
		let cp = TilesConverterParameters {
			tile_format: Some(WEBP),
			format_quality: Some(80),
			tile_compression: Some(Gzip),
			..Default::default()
		};
		let tcr = TilesConvertReader::new_from_reader(reader, cp).await?;

		assert_eq!(tcr.metadata().tile_format(), &WEBP);
		assert_eq!(tcr.metadata().tile_compression(), &Gzip);

		let coord = TileCoord::new(0, 0, 0)?;
		let tile = tcr.tile(&coord).await?.unwrap();
		assert_eq!(tile.format(), WEBP);

		Ok(())
	}

	/// Regression: wrapping a source in a `TilesConvertReader` must not mutate the
	/// source's metadata. Previously the converter would call `set_tile_pyramid`
	/// on a clone whose `Arc<RwLock<...>>` was shared with the source, silently
	/// overwriting the source's pyramid with the swapped/flipped/filtered one.
	/// That broke any subsequent reader of `metadata.intersection_bbox`
	/// (notably `tile_size_stream` → `tile_coord_stream`), so the conversion
	/// progress count came out as 0/1 even though the actual write was correct.
	#[tokio::test]
	async fn swap_xy_does_not_corrupt_source_metadata() -> Result<()> {
		use crate::MBTilesReader;

		// `*.versatiles` is gitignored, so use the tracked mbtiles fixture instead.
		let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.parent()
			.unwrap()
			.join("testdata/berlin.mbtiles");
		let runtime = TilesRuntime::default();
		let reader = MBTilesReader::open(&path, runtime.clone())?;
		let shared = reader.into_shared();

		// Snapshot the source's pyramid before wrapping.
		let before = shared.tile_pyramid().await?.as_ref().clone();

		// Build a converter that swaps X/Y. This used to silently mutate the
		// shared `tile_pyramid` lock.
		let conv = TilesConvertReader::new_from_reader(
			shared.clone(),
			TilesConverterParameters {
				swap_xy: true,
				..Default::default()
			},
		)
		.await?;

		// 1) Source's pyramid must be unchanged.
		let after = shared.tile_pyramid().await?.as_ref().clone();
		assert_eq!(before, after, "source pyramid was mutated by the converter");

		// 2) `tile_coord_stream` and `tile_stream` must agree on counts at every
		//    level. Before the fix this failed for every level except z=0 (which
		//    is invariant under swap) — coord returned 0, stream returned N.
		let pyr = conv.tile_pyramid().await?;
		for z in 0..=pyr.level_max().unwrap_or(0) {
			let bbox = pyr.level_ref(z).to_bbox();
			if bbox.is_empty() {
				continue;
			}
			crate::testing::assert_stream_counts_agree(&conv, bbox).await?;
		}
		Ok(())
	}

	/// Direct exercise of the [`TileSource`](crate::TileSource) stream-count
	/// invariant against the converter, with no swap/flip/filter applied —
	/// catches regressions that wouldn't be specific to the swap_xy path.
	#[tokio::test]
	async fn converter_satisfies_stream_count_invariant() -> Result<()> {
		use crate::MBTilesReader;

		let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.parent()
			.unwrap()
			.join("testdata/berlin.mbtiles");
		let runtime = TilesRuntime::default();
		let shared = MBTilesReader::open(&path, runtime.clone())?.into_shared();

		let conv = TilesConvertReader::new_from_reader(shared, TilesConverterParameters::default()).await?;

		let pyr = conv.tile_pyramid().await?;
		for z in 0..=pyr.level_max().unwrap_or(0) {
			let bbox = pyr.level_ref(z).to_bbox();
			if bbox.is_empty() {
				continue;
			}
			crate::testing::assert_stream_counts_agree(&conv, bbox).await?;
		}
		Ok(())
	}

	// --- tile-count guard (issue #227) ---------------------------------------

	/// A pyramid that reaches the deepest levels describes more tiles than any
	/// machine can index, so it must be refused rather than started.
	#[test]
	fn tile_count_guard_refuses_an_impossible_pyramid() {
		let pyramid = TilePyramid::new_full();

		let err = check_tile_count(&pyramid, DEFAULT_TILE_COUNT_LIMIT).unwrap_err();
		let msg = format!("{err:#}");

		assert!(msg.contains("refusing to convert"), "unexpected message: {msg}");
		// Both numbers must be readable, and the hint must explain itself
		// because the user did not choose this limit.
		assert!(msg.contains("1_537_228_672_809_129_301"), "unexpected message: {msg}");
		assert!(msg.contains("10_000_000_000"), "unexpected message: {msg}");
		assert!(msg.contains("rarely intended"), "unexpected message: {msg}");
	}

	/// A whole-planet tileset is the largest thing anyone routinely builds, and
	/// the default limit exists to stop impossible jobs — not big ones.
	#[rstest]
	#[case::planet_z14(14)]
	#[case::planet_z15(15)]
	fn tile_count_guard_allows_a_planet_sized_pyramid(#[case] level_max: u8) {
		let mut pyramid = TilePyramid::new_full();
		pyramid.set_level_max(level_max);

		assert!(
			check_tile_count(&pyramid, DEFAULT_TILE_COUNT_LIMIT).is_ok(),
			"a planet through zoom {level_max} ({} tiles) must not be refused",
			pyramid.count_tiles()
		);
	}

	/// The boundary is inclusive: exactly `limit` tiles is allowed.
	#[test]
	fn tile_count_guard_boundary_is_inclusive() {
		let mut pyramid = TilePyramid::new_full();
		pyramid.set_level_max(2);
		let count = pyramid.count_tiles();

		assert!(check_tile_count(&pyramid, count).is_ok(), "count == limit must pass");
		assert!(
			check_tile_count(&pyramid, count - 1).is_err(),
			"count > limit must fail"
		);
	}

	/// `VERSATILES_MAX_TILES` overrides the default, and `0` means "no limit".
	///
	/// Serialised because it mutates process-global state, and reads the
	/// variable's prior value so a developer running with it set does not see a
	/// spurious failure.
	#[test]
	fn tile_count_limit_env_var_is_honoured() {
		static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
		let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

		let previous = std::env::var(TILE_COUNT_LIMIT_ENV).ok();
		// SAFETY: serialised by ENV_LOCK; the prior value is restored below.
		let set = |v: Option<&str>| unsafe {
			match v {
				Some(v) => std::env::set_var(TILE_COUNT_LIMIT_ENV, v),
				None => std::env::remove_var(TILE_COUNT_LIMIT_ENV),
			}
		};

		set(None);
		assert_eq!(resolve_tile_count_limit(), DEFAULT_TILE_COUNT_LIMIT, "unset -> default");

		set(Some("500"));
		assert_eq!(resolve_tile_count_limit(), 500, "a number is taken as the limit");

		set(Some("  500  "));
		assert_eq!(resolve_tile_count_limit(), 500, "surrounding whitespace is tolerated");

		set(Some("0"));
		assert_eq!(resolve_tile_count_limit(), u64::MAX, "0 disables the check");

		set(Some("banana"));
		assert_eq!(
			resolve_tile_count_limit(),
			DEFAULT_TILE_COUNT_LIMIT,
			"an unparseable value falls back to the safe default rather than failing"
		);

		set(previous.as_deref());
	}

	/// A user-chosen limit should not be second-guessed in the message.
	#[test]
	fn tile_count_guard_drops_the_hint_when_the_limit_was_chosen() {
		let mut pyramid = TilePyramid::new_full();
		pyramid.set_level_max(2);

		let msg = format!("{:#}", check_tile_count(&pyramid, 5).unwrap_err());
		assert!(!msg.contains("rarely intended"), "unexpected message: {msg}");
		assert!(msg.contains("VERSATILES_MAX_TILES"), "unexpected message: {msg}");
	}

	/// `u64::MAX` is what `VERSATILES_MAX_TILES=0` resolves to, and must let
	/// even the deepest pyramid through.
	#[test]
	fn tile_count_guard_can_be_disabled() {
		assert!(check_tile_count(&TilePyramid::new_full(), u64::MAX).is_ok());
	}

	#[test]
	fn group_digits_inserts_separators() {
		assert_eq!(group_digits(0), "0");
		assert_eq!(group_digits(999), "999");
		assert_eq!(group_digits(1_000), "1_000");
		assert_eq!(group_digits(1_234_567), "1_234_567");
		assert_eq!(group_digits(u64::MAX), "18_446_744_073_709_551_615");
	}
}
