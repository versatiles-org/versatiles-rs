//! # From-csv read operation
//!
//! Reads a CSV file with explicit longitude/latitude columns and exposes
//! it as a [`TileSource`] of MVT vector tiles. Each row becomes a `Point`
//! feature whose properties are the remaining columns (as strings).
//!
//! ## Example
//!
//! ```text
//! from_csv filename="quakes.csv" lon_column="longitude" lat_column="latitude" id_column="event_id" max_zoom=8
//! ```

use crate::{
	PipelineFactory,
	helpers::feature_tile_source::{
		FeatureTileSource, apply_property_filters, parse_compression, parse_point_reduction,
	},
	operations::read::traits::ReadTileSource,
	vpl::VPLNode,
};
use anyhow::{Result, bail};
use futures::StreamExt;
use std::sync::Arc;
use versatiles_container::{DataLocation, TileSource};
use versatiles_derive::context;
use versatiles_geometry::feature_import::{FeatureImport, FeatureImportArgs};
use versatiles_geometry::feature_source::{CsvSourceBuilder, FeatureSource, ProgressCallback};
use versatiles_geometry::geo::GeoFeature;

/// Don't bother showing a progress bar for tiny inputs — the bar would
/// flicker once and disappear. 10 MB is the smallest size where users start
/// to notice the wait.
const PROGRESS_MIN_BYTES: u64 = 10_000_000;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a CSV file with longitude/latitude columns and emits MVT point tiles.
struct Args {
	/// Filename of the CSV file (relative to the VPL file path).
	filename: String,
	/// Header column name holding the longitude (degrees, WGS84). Required.
	lon_column: String,
	/// Header column name holding the latitude (degrees, WGS84). Required.
	lat_column: String,
	/// Optional column to expose as the MVT feature `id` (numeric if it parses as `u64`, else string).
	id_column: Option<String>,
	/// Field delimiter as a single ASCII character. Defaults to `,`.
	delimiter: Option<String>,
	/// Whether row 1 contains column names. Defaults to `true`. Header-less CSVs aren't supported in v1.
	has_header: Option<bool>,
	/// Name of the MVT layer in the output tiles. Defaults to the filename stem.
	layer_name: Option<String>,
	/// Lowest zoom level emitted (default 0).
	min_zoom: Option<u8>,
	/// Highest zoom level emitted. Defaults to an auto-heuristic (median feature
	/// size ≈ 4 tile-pixels, capped at 14). For point-only inputs the heuristic
	/// returns 14.
	max_zoom: Option<u8>,
	/// Bounding-box clip in degrees `[w, s, e, n]`. Not supported in v1; setting
	/// this errors out.
	bbox: Option<[f64; 4]>,
	/// Property whitelist: keep only the named columns as feature properties,
	/// drop everything else. Mutually exclusive with `properties_exclude`.
	/// (`lon_column` / `lat_column` / `id_column` are consumed earlier by the
	/// CSV adapter and aren't affected.)
	properties_include: Option<Vec<String>>,
	/// Property blacklist: drop the named properties, keep everything else.
	/// Mutually exclusive with `properties_include`.
	properties_exclude: Option<Vec<String>>,
	/// Point reduction strategy: `none` / `drop_rate` / `min_distance`
	/// (default `min_distance`).
	point_reduction: Option<String>,
	/// Numeric value whose meaning depends on `point_reduction`:
	/// - `min_distance` (default): minimum distance between kept points,
	///   in tile-pixels at the current zoom. Defaults to 16.
	/// - `drop_rate`: per-zoom keep-fraction in `[0, 1]`. Defaults to 0.5.
	/// - `none`: ignored.
	point_reduction_value: Option<f32>,
	/// Tile-compression applied before the tiles leave this operation:
	/// `gzip` (default), `brotli`, `zstd`, or `none`. Aliases `gz` / `br` /
	/// `zst` / `raw` are accepted.
	compression: Option<String>,
}

/// Marker type for the read-factory macro. The actual runtime `TileSource`
/// is a [`FeatureTileSource`] returned from [`Operation::build`].
pub struct Operation;

impl ReadTileSource for Operation {
	#[context("Failed to build from_csv operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		reject_unsupported_args(&args)?;
		let location = factory.resolve_location(&DataLocation::try_from(args.filename.as_str())?)?;
		let path = location.to_path_buf()?;

		let layer_name = args.layer_name.clone().unwrap_or_else(|| {
			path
				.file_stem()
				.and_then(|s| s.to_str())
				.unwrap_or("features")
				.to_string()
		});

		// Build the CSV source.
		let mut builder = CsvSourceBuilder::new(&path, &args.lon_column, &args.lat_column);
		if let Some(id_col) = &args.id_column {
			builder = builder.id_column(id_col.clone());
		}
		if let Some(delim) = &args.delimiter {
			let bytes = delim.as_bytes();
			if bytes.len() != 1 {
				bail!("delimiter must be exactly one ASCII byte, got '{delim}'");
			}
			builder = builder.delimiter(bytes[0]);
		}
		if let Some(has_header) = args.has_header {
			builder = builder.has_header(has_header);
		}

		// For sources large enough to be visibly slow, attach a byte-level
		// progress bar that ticks as the file is read.
		let total_bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
		let progress_handle = if total_bytes >= PROGRESS_MIN_BYTES {
			let handle = factory.runtime().create_progress("loading CSV", total_bytes);
			let inc = handle.clone();
			let cb: ProgressCallback = Arc::new(move |n| inc.inc(n));
			builder = builder.with_progress(cb);
			Some(handle)
		} else {
			None
		};

		let source = builder.build()?;

		let point_reduction = parse_point_reduction(args.point_reduction.as_deref())?;
		let compression = parse_compression(args.compression.as_deref())?;

		log::info!("from_csv: importing CSV from {}", path.display());
		// Drain features once so the auto-max-zoom heuristic can inspect them.
		let mut stream = source.load()?;
		let mut features: Vec<GeoFeature> = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}
		if let Some(h) = progress_handle {
			h.finish();
		}
		// Apply property filters before passing to FeatureImport so the
		// generated `vector_layers` schema reflects the kept fields.
		apply_property_filters(
			&mut features,
			args.properties_include.as_deref(),
			args.properties_exclude.as_deref(),
		);

		// 1:1 carry-through from VPL args → FeatureImportArgs. CSV is
		// point-only so we don't expose polygon/line knobs; those fields stay
		// `None` and `From<FeatureImportArgs>` fills them with defaults
		// (which are no-ops for point-only input).
		let import_args = FeatureImportArgs {
			layer_name: Some(layer_name.clone()),
			min_zoom: args.min_zoom,
			max_zoom: args.max_zoom,
			point_reduction,
			point_reduction_value: args.point_reduction_value,
			..FeatureImportArgs::default()
		};
		let import = FeatureImport::from_features(features, import_args)?;

		Ok(Box::new(FeatureTileSource::new(
			import,
			&layer_name,
			compression,
			"from_csv",
			"csv features",
			"csv",
		)?) as Box<dyn TileSource>)
	}
}

/// Reject the args we still don't support (`bbox=`) and the combination
/// `properties_include= … properties_exclude=` (ambiguous: pick one).
fn reject_unsupported_args(args: &Args) -> Result<()> {
	if args.bbox.is_some() {
		bail!("from_csv: `bbox=` is not supported");
	}
	if args.properties_include.is_some() && args.properties_exclude.is_some() {
		bail!("from_csv: `properties_include=` and `properties_exclude=` are mutually exclusive");
	}
	Ok(())
}

crate::operations::macros::define_read_factory!("from_csv", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::TileCompression::Uncompressed;
	use versatiles_core::TileCoord;

	#[tokio::test]
	async fn loads_quakes_csv() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"longitude\" lat_column=\"latitude\" \
				 id_column=\"event_id\" layer_name=\"quakes\" max_zoom=8",
			)
			.await?;

		let tile = op.tile(&TileCoord::new(0, 0, 0)?).await?.expect("world tile present");
		let blob = tile.into_blob(&Uncompressed)?;
		assert!(!blob.is_empty());

		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&blob)?;
		assert_eq!(vt.layers[0].name, "quakes");
		assert_eq!(vt.layers[0].features.len(), 3);
		Ok(())
	}

	#[tokio::test]
	async fn missing_lon_column_errors() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"missing\" lat_column=\"latitude\"",
			)
			.await;
		assert!(result.is_err());
		let err_str = format!("{:#}", result.err().unwrap());
		assert!(err_str.contains("missing"), "{err_str}");
	}

	#[tokio::test]
	async fn min_distance_drops_close_points_at_low_zoom() -> Result<()> {
		// At z=0 every tile-pixel is huge (~9.8 km), so a min_distance of 256
		// pixels = ~2500 km easily separates the 3 fixture points (Berlin,
		// Hamburg, München all within ~600 km of each other).
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"longitude\" lat_column=\"latitude\" \
				 max_zoom=8 \
				 point_reduction=\"min_distance\" point_reduction_value=256",
			)
			.await?;

		let tile = op.tile(&TileCoord::new(0, 0, 0)?).await?.expect("world tile");
		let blob = tile.into_blob(&Uncompressed)?;
		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&blob)?;
		// At z=0 the threshold is so wide that only the first point survives.
		assert_eq!(vt.layers[0].features.len(), 1);
		Ok(())
	}

	/// One malformed row in a CSV must not abort the whole load. The skip-
	/// with-warn behavior in `CsvSource::load` propagates through the
	/// `from_csv` operation: surviving rows still become tiles.
	#[tokio::test]
	async fn malformed_row_is_skipped_not_aborted() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("partial.csv");
		std::fs::write(&path, "lon,lat,name\n0.0,0.0,A\nnot_a_number,1.0,B\n2.0,2.0,C\n")?;

		let factory = PipelineFactory::new_dummy();
		// VPL strings treat backslashes as escapes; use forward slashes so the
		// Windows tempdir path (e.g. `C:\Users\…`) survives parsing.
		let path_for_vpl = path.to_string_lossy().replace('\\', "/");
		let vpl = format!("from_csv filename=\"{path_for_vpl}\" lon_column=\"lon\" lat_column=\"lat\" max_zoom=2");
		let op = factory.operation_from_vpl(&vpl).await?;

		let tile = op.tile(&TileCoord::new(0, 0, 0)?).await?.expect("world tile");
		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		assert_eq!(
			vt.layers[0].features.len(),
			2,
			"the two parseable rows should still produce features"
		);
		Ok(())
	}

	#[tokio::test]
	async fn unsupported_args_error() -> Result<()> {
		// `bbox=` is still rejected.
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"longitude\" lat_column=\"latitude\" \
				 bbox=[0,0,1,1]",
			)
			.await;
		assert!(result.is_err());
		let msg = format!("{:#}", result.unwrap_err());
		assert!(msg.contains("bbox"), "{msg}");

		// Combining include + exclude is rejected as ambiguous.
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"longitude\" lat_column=\"latitude\" \
				 properties_include=[\"a\"] properties_exclude=[\"b\"]",
			)
			.await;
		assert!(result.is_err());
		let msg = format!("{:#}", result.unwrap_err());
		assert!(msg.contains("mutually exclusive"), "{msg}");
		Ok(())
	}

	#[tokio::test]
	async fn properties_include_keeps_only_listed() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"longitude\" lat_column=\"latitude\" \
				 properties_include=[\"magnitude\"] max_zoom=2",
			)
			.await?;
		let schema = op.tilejson().vector_layers.0.values().next().unwrap().fields.clone();
		assert_eq!(
			schema.keys().collect::<Vec<_>>(),
			vec!["magnitude"],
			"only `magnitude` should remain after include filter"
		);
		Ok(())
	}

	#[tokio::test]
	async fn properties_exclude_drops_listed() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"longitude\" lat_column=\"latitude\" \
				 properties_exclude=[\"magnitude\"] max_zoom=2",
			)
			.await?;
		let schema = op.tilejson().vector_layers.0.values().next().unwrap().fields.clone();
		assert!(!schema.contains_key("magnitude"), "`magnitude` should be excluded");
		assert!(!schema.is_empty(), "other fields should still be present");
		Ok(())
	}

	#[tokio::test]
	async fn delimiter_must_be_single_byte() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"longitude\" lat_column=\"latitude\" \
				 delimiter=\";;\"",
			)
			.await;
		assert!(result.is_err());
	}
}
