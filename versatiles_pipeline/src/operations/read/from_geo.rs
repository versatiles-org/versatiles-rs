//! # From‑geo read operation
//!
//! Reads a vector geo-data file (GeoJSON, newline-delimited GeoJSON, or
//! Shapefile, dispatched by file extension) and exposes it as a
//! [`TileSource`] of MVT tiles.
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
use std::{path::Path, sync::Arc};
use versatiles_container::{DataLocation, SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileCompression, TileCoord, TileFormat, TileJSON, TilePyramid, TileStream};
use versatiles_derive::context;
use versatiles_geometry::feature_import::{FeatureImport, FeatureImportConfig, PointReductionStrategy};
use versatiles_geometry::feature_source::{FeatureSource, GeoJsonSource, ProgressCallback, ShapefileSource};
use versatiles_geometry::geo::GeoFeature;

/// Don't bother showing a progress bar for tiny inputs — the bar would
/// flicker once and disappear. 10 MB is the smallest size where users start
/// to notice the wait.
const PROGRESS_MIN_BYTES: u64 = 10_000_000;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a GeoJSON or Shapefile and emits MVT vector tiles.
struct Args {
	/// Filename of the input (relative to the VPL file path). Format is detected
	/// from the extension:
	///
	/// - `.geojson` / `.json` — GeoJSON `FeatureCollection`
	/// - `.ndjson` / `.geojsonl` / `.ndgeojson` / `.geojsonseq` — line-delimited
	///   GeoJSON (one feature per line; `.geojsonseq` may use the RFC 8142
	///   record-separator prefix `U+001E`)
	/// - `.shp` — Esri Shapefile
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
	/// Point reduction strategy: `none` / `drop_rate` / `min_distance`
	/// (default `min_distance`, with a 4-tile-pixel threshold).
	point_reduction: Option<String>,
	/// Numeric value whose meaning depends on `point_reduction`. Defaults to
	/// 4 (tile pixels for `min_distance`; ignored for `none`).
	point_reduction_value: Option<f32>,
	/// Tile-compression applied before the tiles leave this operation:
	/// `gzip` (default), `brotli`, `zstd`, or `none`. Aliases `gz` / `br` /
	/// `zst` / `raw` are accepted.
	compression: Option<String>,
}

/// `TileSource` wrapping an in-memory [`FeatureImport`].
pub struct Operation {
	import: Arc<FeatureImport>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	compression: TileCompression,
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

		// Single source of truth for FeatureImport defaults: take the
		// `Default` impl and override only the fields the user actually
		// passed. Avoids repeating the literal default values per op.
		let defaults = FeatureImportConfig::default();
		let point_reduction = args
			.point_reduction
			.as_deref()
			.map(PointReductionStrategy::parse)
			.transpose()?
			.unwrap_or(defaults.point_reduction);

		// Default to gzip — the most widely supported compression for vector
		// tiles; consumers like QGIS, Mapbox GL, and most servers expect it.
		let compression = args
			.compression
			.as_deref()
			.map(TileCompression::try_from)
			.transpose()?
			.unwrap_or(TileCompression::Gzip);

		// Format dispatch by extension (case-insensitive).
		let ext = path
			.extension()
			.and_then(|s| s.to_str())
			.map(str::to_ascii_lowercase)
			.unwrap_or_default();
		let format_label = match ext.as_str() {
			"geojson" | "json" => "GeoJSON",
			"ndjson" | "ndgeojson" | "geojsonl" | "geojsonseq" => "line-delimited GeoJSON",
			"shp" => "Shapefile",
			"" => bail!(
				"file '{}' has no extension; expected .geojson / .json / .ndjson / .geojsonl / .ndgeojson / .geojsonseq / .shp",
				path.display()
			),
			other => bail!("unsupported file extension '.{other}' for from_geo"),
		};
		log::debug!("from_geo: loading {format_label} from {}", path.display());
		let features = load_features(&path, ext.as_str(), format_label, factory).await?;

		// `args.max_zoom` of `None` triggers the auto-heuristic inside
		// `FeatureImport::from_features`; no extra projection pass needed here.
		let config = FeatureImportConfig {
			layer_name: layer_name.clone(),
			min_zoom: args.min_zoom.unwrap_or(defaults.min_zoom),
			max_zoom: args.max_zoom,
			polygon_simplify_px: args.polygon_simplify.unwrap_or(defaults.polygon_simplify_px),
			line_simplify_px: args.line_simplify.unwrap_or(defaults.line_simplify_px),
			polygon_min_area_px: args.polygon_min_area.unwrap_or(defaults.polygon_min_area_px),
			line_min_length_px: args.line_min_length.unwrap_or(defaults.line_min_length_px),
			point_reduction,
			point_reduction_value: args.point_reduction_value.unwrap_or(defaults.point_reduction_value),
		};
		let import = FeatureImport::from_features(features, config)?;

		// Build TileJSON / metadata. Tile pyramid covers the data bbox over
		// [min_zoom, max_zoom]; for empty input, an empty pyramid.
		let pyramid = match import.bounds_geo()? {
			Some(bbox) => TilePyramid::from_geo_bbox(import.min_zoom(), import.max_zoom(), &bbox)?,
			None => TilePyramid::new_empty(),
		};
		let metadata = TileSourceMetadata::new(TileFormat::MVT, compression, Traversal::ANY, Some(pyramid));

		let mut tilejson = TileJSON::default();
		tilejson.set_string("name", &layer_name)?;
		// Vector consumers like QGIS need the TileJSON `vector_layers` entry to
		// know what's in each MVT layer; set one entry covering this layer's
		// fields and zoom range.
		populate_vector_layers(&mut tilejson, &layer_name, &import)?;
		metadata.update_tilejson(&mut tilejson);

		Ok(Box::new(Self {
			import: Arc::new(import),
			metadata,
			tilejson,
			compression,
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
			Some(vector_tile) => {
				let mut tile = Tile::from_vector(vector_tile, TileFormat::MVT)?;
				tile.change_compression(&self.compression)?;
				Ok(Some(tile))
			}
			None => Ok(None),
		}
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("from_geo::tile_stream {bbox:?}");
		let bbox = self.metadata.intersection_bbox(&bbox);
		let import = Arc::clone(&self.import);
		let compression = self.compression;
		Ok(TileStream::from_bbox_parallel(bbox, move |coord| {
			match import.get_tile(coord.level, coord.x, coord.y) {
				Ok(Some(vt)) => {
					let mut tile = Tile::from_vector(vt, TileFormat::MVT).ok()?;
					tile.change_compression(&compression).ok()?;
					Some(tile)
				}
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

/// Build the right [`FeatureSource`] for `ext`, attach a byte-level progress
/// bar when the input is big enough to be visibly slow, and drain it.
async fn load_features(
	path: &Path,
	ext: &str,
	format_label: &str,
	factory: &PipelineFactory,
) -> Result<Vec<GeoFeature>> {
	let total_bytes = source_size_bytes(path, ext);
	let (handle, cb) = if total_bytes >= PROGRESS_MIN_BYTES {
		let handle = factory
			.runtime()
			.create_progress(&format!("loading {format_label}"), total_bytes);
		let inc = handle.clone();
		let cb: ProgressCallback = Arc::new(move |n| inc.inc(n));
		(Some(handle), Some(cb))
	} else {
		(None, None)
	};

	let features = match ext {
		"geojson" | "json" => {
			let mut s = GeoJsonSource::new(path);
			if let Some(cb) = cb {
				s = s.with_progress(cb);
			}
			drain(&s).await?
		}
		"ndjson" | "ndgeojson" | "geojsonl" | "geojsonseq" => {
			let mut s = GeoJsonSource::new_line_delimited(path);
			if let Some(cb) = cb {
				s = s.with_progress(cb);
			}
			drain(&s).await?
		}
		"shp" => {
			let mut s = ShapefileSource::new(path);
			if let Some(cb) = cb {
				s = s.with_progress(cb);
			}
			drain(&s).await?
		}
		_ => unreachable!("caller validated the extension"),
	};
	if let Some(h) = handle {
		h.finish();
	}
	Ok(features)
}

/// Total number of bytes the import will read for the given input. For
/// shapefiles that's the sum of `.shp` + `.dbf` (the .shx is small and
/// the projection file is negligible). Anything we can't stat returns 0,
/// which falls below the progress threshold and silently disables the bar.
fn source_size_bytes(path: &Path, ext: &str) -> u64 {
	let primary = std::fs::metadata(path).map_or(0, |m| m.len());
	if ext == "shp" {
		let dbf = path.with_extension("dbf");
		let dbf_len = std::fs::metadata(&dbf).map_or(0, |m| m.len());
		primary.saturating_add(dbf_len)
	} else {
		primary
	}
}

/// Populate `tilejson.vector_layers` with a single entry describing this
/// import's layer. MBTiles vector consumers (QGIS, Mapbox GL, etc.) read this
/// to discover what's inside the tiles.
fn populate_vector_layers(tilejson: &mut TileJSON, layer_name: &str, import: &FeatureImport) -> Result<()> {
	use versatiles_core::{VectorLayer, VectorLayers};
	let layer = VectorLayer {
		fields: import.property_schema().clone(),
		description: None,
		minzoom: Some(import.min_zoom()),
		maxzoom: Some(import.max_zoom()),
	};
	tilejson.vector_layers = VectorLayers(std::iter::once((layer_name.to_string(), layer)).collect());
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

	/// Newline-delimited GeoJSON: same fixture as `places.geojson`, one
	/// feature per line. Routes through the `LineDelimited` parser path.
	#[tokio::test]
	async fn loads_places_geojsonl() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_geo filename=\"../testdata/places.geojsonl\" layer_name=\"places\" max_zoom=8 \
				 polygon_simplify=0 line_simplify=0 polygon_min_area=0 line_min_length=0",
			)
			.await?;

		let tile = op.tile(&TileCoord::new(0, 0, 0)?).await?.expect("world tile present");
		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		assert_eq!(vt.layers[0].name, "places");
		// Same shape as the FeatureCollection sibling: 4 features → 5 after
		// MultiPolygon flatten.
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

	/// End-to-end topology-preservation test: two adjacent polygons share a
	/// wiggly edge in `testdata/borders.geojson`. After the full pipeline
	/// (project → arc graph → simplify → reassemble → clip → quantize →
	/// encode MVT), the two rendered polygons must still share vertices at
	/// every zoom level — i.e. the intersection of their decoded vertex
	/// sets is non-empty. If topology weren't preserved, simplification
	/// would shift one polygon's edge differently from the other's and the
	/// shared vertices would drift apart.
	#[tokio::test]
	async fn shared_border_topology_preserved_through_from_geo() -> Result<()> {
		use std::collections::HashSet;

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_geo filename=\"../testdata/borders.geojson\" layer_name=\"borders\" \
				 max_zoom=10 polygon_min_area=0 line_min_length=0",
			)
			.await?;

		// The fixture's two polygons share a wiggly edge at lon=6, between lat=2
		// and lat=3. Query the tile that contains that shared edge at each
		// zoom: both polygons must intersect it, so both must show up in the
		// rendered MVT. Topology preservation is the same property at every
		// zoom — checking z=4..=10 is plenty of evidence that the arc-graph
		// pipeline survives end-to-end.
		for z in 4..=10 {
			let coord = versatiles_core::TileCoord::from_geo(6.0, 2.5, z)?;
			let tile = op
				.tile(&coord)
				.await?
				.unwrap_or_else(|| panic!("tile at z={z} present"));
			let blob = tile.into_blob(&Uncompressed)?;
			let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&blob)?;
			assert_eq!(vt.layers[0].features.len(), 2, "both polygons at z={z}");

			let g0 = vt.layers[0].features[0].to_geometry()?;
			let g1 = vt.layers[0].features[1].to_geometry()?;
			let a: HashSet<(i64, i64)> = polygon_vertex_set(&g0);
			let b: HashSet<(i64, i64)> = polygon_vertex_set(&g1);
			assert!(!a.is_empty() && !b.is_empty(), "polygon vertex set empty at z={z}");
			let shared: HashSet<_> = a.intersection(&b).copied().collect();
			assert!(
				!shared.is_empty(),
				"adjacent polygons must still share vertices at z={z} (a={a:?}, b={b:?})"
			);
		}
		Ok(())
	}

	/// Return the integer-grid vertex set of all of a polygon's coordinates.
	fn polygon_vertex_set(g: &geo_types::Geometry<f64>) -> std::collections::HashSet<(i64, i64)> {
		let coords: Vec<geo_types::Coord<f64>> = match g {
			geo_types::Geometry::MultiPolygon(mp) => mp.0.iter().flat_map(|p| p.exterior().0.clone()).collect(),
			geo_types::Geometry::Polygon(p) => p.exterior().0.clone(),
			_ => return std::collections::HashSet::new(),
		};
		coords
			.iter()
			.map(|c| {
				#[allow(clippy::cast_possible_truncation)]
				let q = (c.x.round() as i64, c.y.round() as i64);
				q
			})
			.collect()
	}

	#[tokio::test]
	async fn unsupported_args_error() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let bbox_err = factory
			.operation_from_vpl("from_geo filename=\"../testdata/places.geojson\" bbox=[0,0,1,1]")
			.await;
		assert!(bbox_err.is_err());
		let msg = format!("{:#}", bbox_err.unwrap_err());
		assert!(msg.contains("bbox"), "{msg}");
		Ok(())
	}

	#[tokio::test]
	async fn extension_dispatch_is_case_insensitive() -> Result<()> {
		// Mixed-case `.SHP` should still route to the shapefile path.
		let factory = PipelineFactory::new_dummy();
		// Copy the fixture into a tempdir with an uppercase `.SHP` to test
		// our extension dispatch, but keep the sibling `.shx`/`.dbf` lowercase:
		// the shapefile crate's sidecar lookup is `path.with_extension("dbf")`,
		// which preserves the stem and uses the literal lowercase string we
		// pass — case-sensitive filesystems (Linux CI) require the actual
		// files to match.
		let tmp = tempfile::tempdir()?;
		std::fs::copy("../testdata/admin.shp", tmp.path().join("ADMIN.SHP"))?;
		std::fs::copy("../testdata/admin.shx", tmp.path().join("ADMIN.shx"))?;
		std::fs::copy("../testdata/admin.dbf", tmp.path().join("ADMIN.dbf"))?;
		// VPL strings treat backslashes as escapes; use forward slashes so the
		// Windows tempdir path survives parsing.
		let path_for_vpl = tmp.path().join("ADMIN.SHP").to_string_lossy().replace('\\', "/");
		let vpl = format!("from_geo filename=\"{path_for_vpl}\" max_zoom=4");
		let op = factory.operation_from_vpl(&vpl).await?;
		assert!(op.tile(&TileCoord::new(0, 0, 0)?).await?.is_some());
		Ok(())
	}
}
