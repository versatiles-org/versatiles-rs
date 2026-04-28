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
use std::sync::Arc;
use versatiles_container::{DataLocation, SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;
use versatiles_geometry::feature_import::{FeatureImport, FeatureImportConfig};
use versatiles_geometry::feature_source::CsvSourceBuilder;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
#[allow(dead_code)] // Phase 3 honors a subset of fields; the rest are wired in later phases.
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
	/// Highest zoom level emitted (default 14).
	max_zoom: Option<u8>,
	/// Bounding box [w, s, e, n] in degrees that limits the output (currently
	/// ignored in Phase 1; honored from a later phase).
	bbox: Option<[f64; 4]>,
	/// Property names to keep (currently ignored in Phase 1).
	properties_include: Option<Vec<String>>,
	/// Property names to drop (currently ignored in Phase 1).
	properties_exclude: Option<Vec<String>>,
	/// Drop polygons whose area is below this many tile-pixels² (Phase 4; CSV is points only, so unused here).
	polygon_min_area: Option<f32>,
	/// Douglas-Peucker tolerance for polygons, in tile-pixels (Phase 1; CSV is points only, so unused here).
	polygon_simplify: Option<f32>,
	/// Drop lines whose length is below this many tile-pixels (Phase 4; unused for CSV).
	line_min_length: Option<f32>,
	/// Douglas-Peucker tolerance for lines, in tile-pixels (Phase 1; unused for CSV).
	line_simplify: Option<f32>,
	/// Point reduction strategy: `none` / `drop_rate` / `min_distance` (Phase 4).
	point_reduction: Option<String>,
	/// Numeric value whose meaning depends on `point_reduction` (Phase 4).
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

		let config = FeatureImportConfig {
			layer_name: layer_name.clone(),
			min_zoom: args.min_zoom.unwrap_or(0),
			max_zoom: args.max_zoom.unwrap_or(14),
			polygon_simplify_px: args.polygon_simplify.unwrap_or(4.0),
			line_simplify_px: args.line_simplify.unwrap_or(4.0),
			polygon_min_area_px: args.polygon_min_area.unwrap_or(4.0),
			line_min_length_px: args.line_min_length.unwrap_or(4.0),
		};
		let import = FeatureImport::from_source(&source, config).await?;

		let pyramid = match import.bounds_geo()? {
			Some(bbox) => TilePyramid::from_geo_bbox(import.config().min_zoom, import.config().max_zoom, &bbox)?,
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
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_coord| {
			Some(())
		}))
	}
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
