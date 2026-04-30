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
use std::{path::Path, sync::Arc};
use versatiles_container::{DataLocation, TileSource};
use versatiles_derive::context;
use versatiles_geometry::feature_import::{FeatureImport, FeatureImportArgs};
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
	/// Property whitelist: keep only the named properties, drop everything else.
	/// Mutually exclusive with `properties_exclude`.
	properties_include: Option<Vec<String>>,
	/// Property blacklist: drop the named properties, keep everything else.
	/// Mutually exclusive with `properties_include`.
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
	/// If `true`, drop the GeoJSON / Shapefile `id` field from every feature
	/// before encoding. Useful for sources where the id is a string (e.g. USGS
	/// earthquakes — those would be silently dropped at MVT encode anyway, since
	/// MVT requires `uint64` ids), or when the id is just noise. Defaults to
	/// `false` — keep the id when it's a non-negative integer.
	ignore_id: Option<bool>,
}

/// Marker type for the read-factory macro. The actual runtime `TileSource`
/// is a [`FeatureTileSource`] returned from [`Operation::build`].
pub struct Operation;

impl ReadTileSource for Operation {
	#[context("Failed to build from_geo operation in VPL node {:?}", vpl_node.name)]
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

		let point_reduction = parse_point_reduction(args.point_reduction.as_deref())?;
		let compression = parse_compression(args.compression.as_deref())?;

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
		let mut features = load_features(&path, ext.as_str(), format_label, factory).await?;
		if args.ignore_id.unwrap_or(false) {
			for f in &mut features {
				f.id = None;
			}
		}
		// Apply property filters before passing to FeatureImport so the
		// generated `vector_layers` schema reflects the kept fields.
		apply_property_filters(
			&mut features,
			args.properties_include.as_deref(),
			args.properties_exclude.as_deref(),
		);

		// 1:1 carry-through from VPL args → FeatureImportArgs. `None` fields
		// are filled with defaults inside `FeatureImport::from_features` via
		// the `From<FeatureImportArgs>` impl.
		let import_args = FeatureImportArgs {
			layer_name: Some(layer_name.clone()),
			min_zoom: args.min_zoom,
			max_zoom: args.max_zoom,
			polygon_simplify_px: args.polygon_simplify,
			line_simplify_px: args.line_simplify,
			polygon_min_area_px: args.polygon_min_area,
			line_min_length_px: args.line_min_length,
			point_reduction,
			point_reduction_value: args.point_reduction_value,
		};
		let import = FeatureImport::from_features(features, import_args)?;

		Ok(Box::new(FeatureTileSource::new(
			import,
			&layer_name,
			compression,
			"from_geo",
			"geo features",
			"geo",
		)?) as Box<dyn TileSource>)
	}
}

/// Reject the args we still don't support (`bbox=`) and the combination
/// `properties_include= … properties_exclude=` (ambiguous: pick one).
fn reject_unsupported_args(args: &Args) -> Result<()> {
	if args.bbox.is_some() {
		bail!("from_geo: `bbox=` is not supported");
	}
	if args.properties_include.is_some() && args.properties_exclude.is_some() {
		bail!("from_geo: `properties_include=` and `properties_exclude=` are mutually exclusive");
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
	use versatiles_core::TileCoord;

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
	async fn ignore_id_strips_feature_ids() -> Result<()> {
		// places.geojson features carry integer ids (1..=4). Verify the
		// default keeps them and `ignore_id=true` drops them all.
		const COMMON: &str = "layer_name=\"places\" max_zoom=8 polygon_simplify=0 line_simplify=0 \
			 polygon_min_area=0 line_min_length=0";

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!("from_geo filename=\"../testdata/places.geojson\" {COMMON}"))
			.await?;
		let tile = op.tile(&TileCoord::new(0, 0, 0)?).await?.expect("world tile present");
		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		assert!(
			vt.layers[0].features.iter().any(|f| f.id.is_some()),
			"default: at least one feature should carry its id"
		);

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_geo filename=\"../testdata/places.geojson\" ignore_id=true {COMMON}"
			))
			.await?;
		let tile = op.tile(&TileCoord::new(0, 0, 0)?).await?.expect("world tile present");
		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		assert!(
			vt.layers[0].features.iter().all(|f| f.id.is_none()),
			"ignore_id=true: every feature should have id=None"
		);
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
		// `bbox=` is still rejected.
		let factory = PipelineFactory::new_dummy();
		let bbox_err = factory
			.operation_from_vpl("from_geo filename=\"../testdata/places.geojson\" bbox=[0,0,1,1]")
			.await;
		assert!(bbox_err.is_err());
		let msg = format!("{:#}", bbox_err.unwrap_err());
		assert!(msg.contains("bbox"), "{msg}");

		// Combining include + exclude is rejected as ambiguous.
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				"from_geo filename=\"../testdata/places.geojson\" \
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
		// places.geojson features have `name` and `kind` — keep only `name`.
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_geo filename=\"../testdata/places.geojson\" max_zoom=4 \
				 properties_include=[\"name\"]",
			)
			.await?;
		let schema = op.tilejson().vector_layers.0.values().next().unwrap().fields.clone();
		assert_eq!(schema.keys().collect::<Vec<_>>(), vec!["name"]);
		Ok(())
	}

	#[tokio::test]
	async fn properties_exclude_drops_listed() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_geo filename=\"../testdata/places.geojson\" max_zoom=4 \
				 properties_exclude=[\"kind\"]",
			)
			.await?;
		let schema = op.tilejson().vector_layers.0.values().next().unwrap().fields.clone();
		assert!(!schema.contains_key("kind"));
		assert!(schema.contains_key("name"));
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
