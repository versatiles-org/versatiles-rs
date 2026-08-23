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

use std::sync::Arc;

use anyhow::{Result, bail, ensure};
use futures::StreamExt;
use versatiles_container::{DataLocation, TileSource};
use versatiles_core::TileCompression;
use versatiles_derive::context;
use versatiles_geometry::{
	feature_import::{FeatureImport, FeatureImportArgs, PointReductionStrategy},
	feature_source::{CsvSourceBuilder, FeatureSource, ProgressCallback},
	geo::GeoFeature,
};

use crate::{
	PipelineFactory,
	helpers::{
		feature_tile_source::{BBoxClip, FeatureTileSource, FeatureTileSourceArgs, apply_property_filters},
		tile_size_monitor::MaxTileBytes,
	},
	vpl::VPLNode,
};

/// Don't bother showing a progress bar for tiny inputs — the bar would
/// flicker once and disappear. 10 MB is the smallest size where users start
/// to notice the wait.
const PROGRESS_MIN_BYTES: u64 = 10_000_000;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a CSV file with longitude and latitude columns and emits MVT point tiles.
///
/// Left to itself, `max_zoom` picks the level at which the median feature spans
/// roughly 4 tile-pixels, capped at 14. Points count as size zero, so a CSV of
/// points always lands on the cap.
///
/// `properties_include` and `properties_exclude` act on what is left after the
/// coordinate and id columns are consumed, so naming `lon_column`,
/// `lat_column` or `id_column` in either has no effect.
///
/// A tile over `max_tile_bytes` is dropped while streaming and an error when a
/// single tile is requested. Raise the cap when a legitimate low-zoom tile
/// exceeds it, or set `max_tile_bytes=none` to emit tiles at any size; the
/// soft-cap warning threshold, 200 KB at the default, scales with it.
///
/// `bbox` is `[west, south, east, north]` in degrees, and crops while reading:
/// rows outside it are dropped before anything is built, and the tile pyramid is
/// clipped to it. That turns a large CSV into one region's tiles in a single
/// pass, where cropping afterwards with `versatiles convert --bbox` means
/// writing the whole thing out first. The crop is tile-granular, as it is there.
struct Args {
	/// Path to the CSV file.
	filename: String,
	/// Column holding the longitude, in WGS84 degrees.
	lon_column: String,
	/// Column holding the latitude, in WGS84 degrees.
	lat_column: String,
	/// Column to expose as the feature id. Defaults to emitting no id.
	id_column: Option<String>,
	/// Character separating a row's fields. Defaults to `,`.
	#[vpl(default = ",")]
	delimiter: Option<String>,
	/// Whether the first row holds column names; `false` is not supported yet. Defaults to `true`.
	#[vpl(default = "true")]
	has_header: Option<bool>,
	/// Name of the layer to write into. Defaults to the file's stem.
	layer_name: Option<String>,
	/// Lowest zoom level to emit. Defaults to `0`.
	#[vpl(default = "0")]
	min_zoom: Option<u8>,
	/// Highest zoom level to emit. Defaults to a heuristic capped at `14`.
	max_zoom: Option<u8>,
	/// Area to restrict the output to, in WGS84 degrees. Defaults to the input's extent.
	bbox: Option<[f64; 4]>,
	/// Columns to keep as properties. Mutually exclusive with `properties_exclude`. Defaults to all.
	properties_include: Option<Vec<String>>,
	/// Columns to drop. Mutually exclusive with `properties_include`. Defaults to none.
	properties_exclude: Option<Vec<String>>,
	/// How to thin out points too close to distinguish. Defaults to `min_distance`.
	#[vpl(default = "min_distance")]
	point_reduction: Option<PointReductionStrategy>,
	/// Distance in tile-pixels for `min_distance`, keep-fraction for `drop_rate`. Defaults to
	/// `16`/`0.5`.
	point_reduction_value: Option<f32>,
	/// Compression applied before the tiles leave. Defaults to `gzip`.
	#[vpl(default = "gzip")]
	compression: Option<TileCompression>,
	/// Size in bytes above which a tile counts as broken. Defaults to `1048576`.
	#[vpl(default = "1048576")]
	max_tile_bytes: Option<MaxTileBytes>,
}

/// Marker type for the read-factory macro. The actual runtime `TileSource`
/// is a [`FeatureTileSource`] returned from [`Operation::build`].
pub struct Operation;

impl Operation {
	#[context("Failed to build from_csv operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>> {
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

		let point_reduction = args.point_reduction;
		let compression = args.compression.unwrap_or(TileCompression::Gzip);
		let clip = args.bbox.map(BBoxClip::new).transpose()?;

		log::info!("from_csv: importing CSV from {}", path.display());
		// Drain features once so the auto-max-zoom heuristic can inspect them.
		// Project + flatten on the fly so we don't make two extra full-Vec
		// passes after load. CSV is point-only so flatten is effectively a
		// no-op, but project_and_flatten keeps the call site uniform with
		// from_geo.
		use versatiles_geometry::feature_import::project_and_flatten;
		let mut stream = source.load()?;
		let mut features: Vec<GeoFeature> = Vec::new();
		while let Some(item) = stream.next().await {
			// A `bbox=` filters here rather than afterwards: on a large input
			// cropped to a small area the dropped rows are nearly all of them,
			// and never accumulating them is the point.
			features.extend(
				project_and_flatten(item?)?
					.into_iter()
					.filter(|f| clip.as_ref().is_none_or(|c| c.keeps(f))),
			);
		}
		if let Some(h) = progress_handle {
			h.finish();
		}
		// A clip that keeps nothing is worth saying out loud: the alternative is
		// `FeatureImport` reporting that it could not compute bounds, which reads
		// like a broken file rather than a bbox that missed.
		if let Some(clip) = &clip {
			ensure!(
				!features.is_empty(),
				"bbox={:?} selects no features from {}",
				clip.geo().as_tuple(),
				path.display()
			);
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
			&FeatureTileSourceArgs {
				layer_name: &layer_name,
				compression,
				label: "from_csv",
				source_description: "csv features",
				source_short: "csv",
				max_tile_bytes: args.max_tile_bytes,
				clip,
			},
		)?) as Box<dyn TileSource>)
	}
}

/// Reject the argument combination `properties_include= … properties_exclude=`
/// (ambiguous: pick one).
fn reject_unsupported_args(args: &Args) -> Result<()> {
	if args.properties_include.is_some() && args.properties_exclude.is_some() {
		bail!("from_csv: `properties_include=` and `properties_exclude=` are mutually exclusive");
	}
	Ok(())
}

crate::operations::macros::define_read_factory!("from_csv", Args, Operation);

#[cfg(test)]
mod tests {
	use versatiles_core::{TileCompression::Uncompressed, TileCoord};

	use super::*;

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
	async fn max_tile_bytes_propagates_to_single_tile_path() -> Result<()> {
		const COMMON: &str = "lon_column=\"longitude\" lat_column=\"latitude\" layer_name=\"quakes\" max_zoom=8";

		// A 1-byte cap makes the world tile over-cap: the single-tile API must
		// surface the error instead of silently returning no tile.
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_csv filename=\"../testdata/quakes.csv\" max_tile_bytes=1 {COMMON}"
			))
			.await?;
		let err = op.tile(&TileCoord::new(0, 0, 0)?).await.unwrap_err();
		assert!(format!("{err:#}").contains("hard cap"), "{err:#}");

		// `max_tile_bytes=none` disables the hard cap: the same tile is emitted.
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_csv filename=\"../testdata/quakes.csv\" max_tile_bytes=none {COMMON}"
			))
			.await?;
		assert!(
			op.tile(&TileCoord::new(0, 0, 0)?).await?.is_some(),
			"disabled hard cap must emit the world tile"
		);
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

	/// `bbox=` keeps only the rows inside it, and bounds the output to it.
	///
	/// The fixture has one point over Berlin, one over Hamburg and one over
	/// Munich; a box around Berlin alone must leave one feature and a pyramid
	/// that no longer reaches the others.
	#[tokio::test]
	async fn bbox_keeps_only_the_rows_inside_it() -> Result<()> {
		const FIXTURE: &str = "from_csv filename=\"../testdata/quakes.csv\" \
			 lon_column=\"longitude\" lat_column=\"latitude\" max_zoom=8";

		let cropped = PipelineFactory::new_dummy()
			.operation_from_vpl(&format!("{FIXTURE} bbox=[13.0,52.3,13.8,52.7]"))
			.await?;

		let coord = TileCoord::from_geo(13.405, 52.52, 8)?;
		let tile = cropped.tile(&coord).await?.expect("tile over Berlin");
		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		assert_eq!(vt.layers[0].features.len(), 1);

		// Hamburg is ~3.4° west and 1° north: outside the box, and now outside the
		// pyramid as well.
		let hamburg = TileCoord::from_geo(9.9937, 53.5511, 8)?;
		assert!(cropped.tile(&hamburg).await?.is_none());

		let whole = PipelineFactory::new_dummy().operation_from_vpl(FIXTURE).await?;
		assert!(
			whole.tile(&hamburg).await?.is_some(),
			"without a bbox Hamburg is still there"
		);
		Ok(())
	}

	/// A box that misses every row says so, rather than reporting unusable bounds.
	#[tokio::test]
	async fn a_bbox_that_selects_nothing_says_so() {
		let msg = format!(
			"{:#}",
			PipelineFactory::new_dummy()
				.operation_from_vpl(
					"from_csv filename=\"../testdata/quakes.csv\" \
					 lon_column=\"longitude\" lat_column=\"latitude\" bbox=[0,0,1,1]"
				)
				.await
				.expect_err("a box over the Atlantic should be refused")
		);
		assert!(msg.contains("selects no features"), "{msg}");
	}

	#[tokio::test]
	async fn unsupported_args_error() -> Result<()> {
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
