//! # From‑geo read operation
//!
//! Reads a vector geo-data file (GeoJSON or Shapefile, dispatched by file
//! extension) and exposes it as a [`TileSource`] of MVT tiles.
//!
//! Internally builds a [`FeatureImport`] which projects features to web
//! mercator, decomposes shared boundaries into an arc graph, simplifies
//! per zoom (topology-preserving), applies geometry-typed reduction, and
//! renders tiles on demand by clipping + quantizing.
//!
//! ## Example
//!
//! ```text
//! from_geo filename="places.geojson" layer_name="places" max_zoom=12
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
use versatiles_geometry::feature_source::{FeatureSource, GeoJsonSource, ShapefileSource};
use versatiles_geometry::geo::GeoFeature;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a GeoJSON or Shapefile and emits MVT vector tiles.
struct Args {
	/// Filename of the GeoJSON or Shapefile (relative to the VPL file path).
	/// Format is detected from the extension (`.geojson`, `.json`, `.ndjson`, `.shp`).
	filename: String,
	/// Name of the MVT layer in the output tiles. Defaults to the filename stem.
	layer_name: Option<String>,
	/// Lowest zoom level emitted (default 0).
	min_zoom: Option<u8>,
	/// Highest zoom level emitted. Defaults to an auto-heuristic (median feature
	/// size ≈ 4 tile-pixels, capped at 14).
	max_zoom: Option<u8>,
	/// Bounding-box clip in degrees `[w, s, e, n]`. Not supported in v1; setting
	/// this errors out.
	bbox: Option<[f64; 4]>,
	/// Property whitelist. Not supported in v1; setting this errors out.
	properties_include: Option<Vec<String>>,
	/// Property blacklist. Not supported in v1; setting this errors out.
	properties_exclude: Option<Vec<String>>,
	/// Drop polygons whose area is below this many tile-pixels² (default 4).
	polygon_min_area: Option<f32>,
	/// Douglas-Peucker tolerance for polygons, in tile-pixels (default 4).
	polygon_simplify: Option<f32>,
	/// Drop lines whose length is below this many tile-pixels (default 4).
	line_min_length: Option<f32>,
	/// Douglas-Peucker tolerance for lines, in tile-pixels (default 4).
	line_simplify: Option<f32>,
	/// Point reduction strategy: `none` / `drop_rate` / `min_distance` (default `none`).
	point_reduction: Option<String>,
	/// Numeric value whose meaning depends on `point_reduction`.
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

		let point_reduction = args
			.point_reduction
			.as_deref()
			.map(PointReductionStrategy::parse)
			.transpose()?
			.unwrap_or_default();

		// Format dispatch by extension (case-insensitive).
		let ext = path
			.extension()
			.and_then(|s| s.to_str())
			.map(str::to_ascii_lowercase)
			.unwrap_or_default();
		let features: Vec<GeoFeature> = match ext.as_str() {
			"geojson" | "json" | "ndjson" => drain(&GeoJsonSource::new(&path)).await?,
			"shp" => drain(&ShapefileSource::new(&path)).await?,
			"" => bail!(
				"file '{}' has no extension; expected .geojson / .json / .ndjson / .shp",
				path.display()
			),
			other => bail!("unsupported file extension '.{other}' for from_geo"),
		};

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

		// Build TileJSON / metadata. Tile pyramid covers the data bbox over
		// [min_zoom, max_zoom]; for empty input, an empty pyramid.
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
		bail!("from_geo: `bbox=` is not supported in v1");
	}
	if args.properties_include.is_some() {
		bail!("from_geo: `properties_include=` is not supported in v1");
	}
	if args.properties_exclude.is_some() {
		bail!("from_geo: `properties_exclude=` is not supported in v1");
	}
	Ok(())
}

/// Drain a `FeatureSource`'s stream into a `Vec`.
async fn drain<S: FeatureSource + ?Sized>(source: &S) -> Result<Vec<GeoFeature>> {
	let mut stream = source.load()?;
	let mut features = Vec::new();
	while let Some(item) = stream.next().await {
		features.push(item?);
	}
	Ok(features)
}

crate::operations::macros::define_read_factory!("from_geo", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::TileCompression::Uncompressed;

	#[tokio::test]
	async fn loads_places_geojson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// Disable simplification + reduction so the small fixture features all
		// survive at z=0. (Phase 5's per-zoom DP collapses small polygons at low
		// zoom — a deliberate v1 behaviour, but unwanted for this load smoke test.)
		let op = factory
			.operation_from_vpl(
				"from_geo filename=\"../testdata/places.geojson\" layer_name=\"places\" max_zoom=8 \
				 polygon_simplify=0 line_simplify=0 polygon_min_area=0 line_min_length=0",
			)
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
