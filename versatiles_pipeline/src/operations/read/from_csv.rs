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

use crate::{PipelineFactory, operations::read::traits::ReadTileSource, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::StreamExt;
use std::sync::Arc;
use versatiles_container::{DataLocation, SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;
use versatiles_geometry::feature_import::{FeatureImport, FeatureImportConfig, PointReductionStrategy};
use versatiles_geometry::feature_source::{CsvSourceBuilder, FeatureSource};
use versatiles_geometry::geo::GeoFeature;

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
	/// Property whitelist. Not supported in v1; setting this errors out.
	properties_include: Option<Vec<String>>,
	/// Property blacklist. Not supported in v1; setting this errors out.
	properties_exclude: Option<Vec<String>>,
	/// Has no effect for CSV (point-only input).
	polygon_min_area: Option<f32>,
	/// Has no effect for CSV (point-only input).
	polygon_simplify: Option<f32>,
	/// Has no effect for CSV (point-only input).
	line_min_length: Option<f32>,
	/// Has no effect for CSV (point-only input).
	line_simplify: Option<f32>,
	/// Point reduction strategy: `none` / `drop_rate` / `min_distance` (default `none`).
	point_reduction: Option<String>,
	/// Numeric value whose meaning depends on `point_reduction`.
	point_reduction_value: Option<f32>,
}

/// `TileSource` wrapping an in-memory [`FeatureImport`] built from a CSV source.
pub struct Operation {
	import: Arc<FeatureImport>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl std::fmt::Debug for Operation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("from_csv::Operation")
			.field("metadata", &self.metadata)
			.finish()
	}
}

impl ReadTileSource for Operation {
	#[context("Failed to build from_csv operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
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
		let source = builder.build()?;

		let point_reduction = args
			.point_reduction
			.as_deref()
			.map(PointReductionStrategy::parse)
			.transpose()?
			.unwrap_or_default();

		// Drain features once so the auto-max-zoom heuristic can inspect them.
		let mut stream = source.load()?;
		let mut features: Vec<GeoFeature> = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}

		// `args.max_zoom` of `None` triggers the auto-heuristic inside
		// `FeatureImport::from_features`; no extra projection pass needed here.
		let config = FeatureImportConfig {
			layer_name: layer_name.clone(),
			min_zoom: args.min_zoom.unwrap_or(0),
			max_zoom: args.max_zoom,
			polygon_simplify_px: args.polygon_simplify.unwrap_or(4.0),
			line_simplify_px: args.line_simplify.unwrap_or(4.0),
			polygon_min_area_px: args.polygon_min_area.unwrap_or(4.0),
			line_min_length_px: args.line_min_length.unwrap_or(4.0),
			point_reduction,
			point_reduction_value: args.point_reduction_value.unwrap_or(0.0),
		};
		let import = FeatureImport::from_features(features, config)?;

		let pyramid = match import.bounds_geo()? {
			Some(bbox) => TilePyramid::from_geo_bbox(import.min_zoom(), import.max_zoom(), &bbox)?,
			None => TilePyramid::new_empty(),
		};
		let metadata = TileSourceMetadata::new(
			TileFormat::MVT,
			TileCompression::Uncompressed,
			Traversal::ANY,
			Some(pyramid),
		);

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", &layer_name)?;
		metadata.update_tilejson(&mut tilejson);

		Ok(Box::new(Self {
			import: Arc::new(import),
			metadata,
			tilejson,
		}) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("csv features", "csv")
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self
			.metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow::anyhow!("tile_pyramid not set"))
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		match self.import.get_tile(coord.level, coord.x, coord.y)? {
			Some(vector_tile) => Ok(Some(Tile::from_vector(vector_tile, TileFormat::MVT)?)),
			None => Ok(None),
		}
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("from_csv::tile_stream {bbox:?}");
		let bbox = self.metadata.intersection_bbox(&bbox);
		let import = Arc::clone(&self.import);
		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			match import.get_tile(coord.level, coord.x, coord.y) {
				Ok(Some(vt)) => Tile::from_vector(vt, TileFormat::MVT).ok(),
				_ => None,
			}
		}))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		// Yield only coords that the source's pyramid actually covers — for a
		// small dataset at high `max_zoom`, this avoids reporting millions of
		// empty coords through the pipeline.
		let pyramid = self.metadata.tile_pyramid();
		Ok(TileStream::from_iter_coord(
			bbox.into_iter_coords(),
			move |coord| match &pyramid {
				Some(p) if !p.includes_coord(&coord) => None,
				_ => Some(()),
			},
		))
	}
}

/// Reject the v1-deferred args (bbox / properties_include / properties_exclude) with a
/// clear error message when they're set, instead of silently no-oping.
fn reject_unsupported_args(args: &Args) -> Result<()> {
	if args.bbox.is_some() {
		bail!("from_csv: `bbox=` is not supported in v1");
	}
	if args.properties_include.is_some() {
		bail!("from_csv: `properties_include=` is not supported in v1");
	}
	if args.properties_exclude.is_some() {
		bail!("from_csv: `properties_exclude=` is not supported in v1");
	}
	Ok(())
}

crate::operations::macros::define_read_factory!("from_csv", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::TileCompression::Uncompressed;

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
		std::fs::write(
			&path,
			"lon,lat,name\n0.0,0.0,A\nnot_a_number,1.0,B\n2.0,2.0,C\n",
		)?;

		let factory = PipelineFactory::new_dummy();
		let vpl = format!(
			"from_csv filename=\"{}\" lon_column=\"lon\" lat_column=\"lat\" max_zoom=2",
			path.display()
		);
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
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				"from_csv filename=\"../testdata/quakes.csv\" \
				 lon_column=\"longitude\" lat_column=\"latitude\" \
				 properties_include=[\"name\"]",
			)
			.await;
		assert!(result.is_err());
		let msg = format!("{:#}", result.unwrap_err());
		assert!(msg.contains("properties_include"), "{msg}");
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
