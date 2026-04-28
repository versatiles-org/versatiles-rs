//! # From‑geo read operation
//!
//! Reads a vector geo-data file (GeoJSON in Phase 1; Shapefile added later)
//! and exposes it as a [`TileSource`] of MVT tiles.
//!
//! Internally builds a [`FeatureImport`] which projects features to web
//! mercator, simplifies per zoom, indexes with an R-tree, then renders
//! tiles on demand by clipping + quantizing.
//!
//! Phase 1 only honors `filename`, `layer_name`, `min_zoom`, `max_zoom`,
//! `polygon_simplify`, `line_simplify`. Other args parse but are no-ops
//! until later phases.
//!
//! ## Example
//!
//! ```text
//! from_geo filename="places.geojson" layer_name="places" max_zoom=12
//! ```

use crate::{PipelineFactory, operations::read::traits::ReadTileSource, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::{DataLocation, SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;
use versatiles_geometry::feature_import::{FeatureImport, FeatureImportConfig};
use versatiles_geometry::feature_source::{GeoJsonSource, ShapefileSource};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
#[allow(dead_code)] // Phase 1 honors a subset of fields; the rest are wired in later phases.
/// Reads a GeoJSON or Shapefile and emits MVT vector tiles.
struct Args {
	/// Filename of the GeoJSON or Shapefile (relative to the VPL file path).
	/// Format is detected from the extension (`.geojson`, `.json`, `.ndjson`, `.shp`).
	filename: String,
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
	/// Drop polygons whose area is below this many tile-pixels² (Phase 4).
	polygon_min_area: Option<f32>,
	/// Douglas-Peucker tolerance for polygons, in tile-pixels (default 4).
	polygon_simplify: Option<f32>,
	/// Drop lines whose length is below this many tile-pixels (Phase 4).
	line_min_length: Option<f32>,
	/// Douglas-Peucker tolerance for lines, in tile-pixels (default 4).
	line_simplify: Option<f32>,
	/// Point reduction strategy: `none` / `drop_rate` / `min_distance` (Phase 4).
	point_reduction: Option<String>,
	/// Numeric value whose meaning depends on `point_reduction` (Phase 4).
	point_reduction_value: Option<f32>,
}

/// `TileSource` wrapping an in-memory [`FeatureImport`].
pub struct Operation {
	import: Arc<FeatureImport>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
}

impl std::fmt::Debug for Operation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("from_geo::Operation")
			.field("metadata", &self.metadata)
			.finish()
	}
}

impl ReadTileSource for Operation {
	#[context("Failed to build from_geo operation in VPL node {:?}", vpl_node.name)]
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

		let config = FeatureImportConfig {
			layer_name: layer_name.clone(),
			min_zoom: args.min_zoom.unwrap_or(0),
			max_zoom: args.max_zoom.unwrap_or(14),
			polygon_simplify_px: args.polygon_simplify.unwrap_or(4.0),
			line_simplify_px: args.line_simplify.unwrap_or(4.0),
		};

		// Format dispatch by extension (case-insensitive).
		let ext = path
			.extension()
			.and_then(|s| s.to_str())
			.map(str::to_ascii_lowercase)
			.unwrap_or_default();
		let import = match ext.as_str() {
			"geojson" | "json" | "ndjson" => {
				let source = GeoJsonSource::new(&path);
				FeatureImport::from_source(&source, config).await?
			}
			"shp" => {
				let source = ShapefileSource::new(&path);
				FeatureImport::from_source(&source, config).await?
			}
			"" => bail!(
				"file '{}' has no extension; expected .geojson / .json / .ndjson / .shp",
				path.display()
			),
			other => bail!("unsupported file extension '.{other}' for from_geo"),
		};

		// Build TileJSON / metadata. Tile pyramid covers the data bbox over
		// [min_zoom, max_zoom]; for empty input, an empty pyramid.
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
		SourceType::new_container("geo features", "geo")
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
		log::trace!("from_geo::tile_stream {bbox:?}");
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

crate::operations::macros::define_read_factory!("from_geo", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::TileCompression::Uncompressed;

	#[tokio::test]
	async fn loads_places_geojson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_geo filename=\"../testdata/places.geojson\" layer_name=\"places\" max_zoom=8")
			.await?;

		// Tile (0, 0, 0) covers the whole world; 4 fixture features → 5 after
		// MultiPolygon flatten.
		let tile = op.tile(&TileCoord::new(0, 0, 0)?).await?.expect("world tile present");
		let blob = tile.into_blob(&Uncompressed)?;
		assert!(!blob.is_empty());

		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&blob)?;
		assert_eq!(vt.layers[0].name, "places");
		assert_eq!(vt.layers[0].features.len(), 5);
		Ok(())
	}

	#[tokio::test]
	async fn unsupported_extension_errors() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_geo filename=\"../testdata/places.txt\"")
			.await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn loads_admin_shapefile() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_geo filename=\"../testdata/admin.shp\" layer_name=\"admin\" max_zoom=8")
			.await?;

		// Tile (0, 0, 0) covers the whole world; both polygons (Berlin + Brandenburg)
		// should be present.
		let tile = op.tile(&TileCoord::new(0, 0, 0)?).await?.expect("world tile present");
		let blob = tile.into_blob(&Uncompressed)?;
		assert!(!blob.is_empty());

		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&blob)?;
		assert_eq!(vt.layers[0].name, "admin");
		assert_eq!(vt.layers[0].features.len(), 2);
		Ok(())
	}

	#[tokio::test]
	async fn extension_dispatch_is_case_insensitive() -> Result<()> {
		// Mixed-case `.SHP` should still route to the shapefile path.
		let factory = PipelineFactory::new_dummy();
		// Symlink `admin.SHP` → `admin.shp` so both the .shp/.shx/.dbf lookups
		// resolve. We do this in a tempdir to keep the testdata directory clean.
		let tmp = tempfile::tempdir()?;
		for ext in ["shp", "shx", "dbf"] {
			let upper = ext.to_uppercase();
			std::fs::copy(
				format!("../testdata/admin.{ext}"),
				tmp.path().join(format!("ADMIN.{upper}")),
			)?;
		}
		let path_str = tmp.path().join("ADMIN.SHP");
		let vpl = format!("from_geo filename=\"{}\" max_zoom=4", path_str.display());
		let op = factory.operation_from_vpl(&vpl).await?;
		assert!(op.tile(&TileCoord::new(0, 0, 0)?).await?.is_some());
		Ok(())
	}
}
