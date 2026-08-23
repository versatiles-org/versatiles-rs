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

use std::{path::Path, sync::Arc};

use anyhow::{Result, bail, ensure};
use futures::StreamExt;
use versatiles_container::{DataLocation, TileSource};
use versatiles_core::TileCompression;
use versatiles_derive::context;
use versatiles_geometry::{
	feature_import::{FeatureImport, FeatureImportArgs, PointReductionStrategy},
	feature_source::{FeatureSource, GeoJsonSource, ProgressCallback, ShapefileSource},
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
/// Reads a GeoJSON or Shapefile and emits MVT vector tiles.
///
/// The input format is detected from the file extension:
///
/// | Extension                                          | Format                                |
/// | -------------------------------------------------- | ------------------------------------- |
/// | `.geojson`, `.json`                                | GeoJSON `FeatureCollection`           |
/// | `.ndjson`, `.geojsonl`, `.ndgeojson`, `.geojsonseq` | line-delimited GeoJSON, one feature per line |
/// | `.shp`                                             | Esri Shapefile                        |
///
/// A `.geojsonseq` file may prefix each record with the RFC 8142 record
/// separator `U+001E`.
///
/// Input must be **EPSG:4326 lon/lat in degrees, longitude first** —
/// reprojection is not performed. Coordinates outside that range are refused,
/// as is a GeoJSON `crs` member naming another projection, but lat/lon input
/// with the axes swapped is in range and cannot be detected: it produces valid
/// tiles of the wrong place. Reproject first if needed, e.g.
/// `ogr2ogr -t_srs EPSG:4326 out.geojson in.geojson`. A shapefile without a
/// `.prj` is read as WGS84 with a warning, since it carries no CRS of its own.
///
/// Left to itself, `max_zoom` picks the level at which the median feature spans
/// roughly 4 tile-pixels, capped at 14. Points count as size zero, so a
/// mostly-point dataset lands on the cap.
///
/// A tile over `max_tile_bytes` is dropped while streaming and an error when a
/// single tile is requested. Raise the cap when a legitimate low-zoom tile
/// exceeds it, or set `max_tile_bytes=none` to emit tiles at any size; the
/// soft-cap warning threshold, 200 KB at the default, scales with it.
///
/// `bbox` is `[west, south, east, north]` in degrees, and crops while reading:
/// features outside it are dropped before anything is built, and the tile
/// pyramid is clipped to it. That turns a planet-scale extract into one city's
/// tiles in a single pass, where cropping afterwards with
/// `versatiles convert --bbox` means writing the whole thing out first. The crop
/// is tile-granular, as it is there — a boundary tile keeps whatever falls
/// inside it, including the parts of a feature that reach past the box.
///
/// `ignore_id` exists because MVT requires a `uint64` feature id. A string id —
/// as USGS earthquake data has — is dropped at encode time anyway, so setting
/// this makes that explicit; it is also the way to discard an id that is just
/// noise.
struct Args {
	/// Path to the input file; its format comes from the extension.
	filename: String,
	/// Name of the layer to write into. Defaults to the file's stem.
	layer_name: Option<String>,
	/// Lowest zoom level to emit. Defaults to `0`.
	#[vpl(default = "0")]
	min_zoom: Option<u8>,
	/// Highest zoom level to emit. Defaults to a heuristic capped at `14`.
	max_zoom: Option<u8>,
	/// Area to restrict the output to, in WGS84 degrees. Defaults to the input's extent.
	bbox: Option<[f64; 4]>,
	/// Properties to keep. Mutually exclusive with `properties_exclude`. Defaults to all.
	properties_include: Option<Vec<String>>,
	/// Properties to drop. Mutually exclusive with `properties_include`. Defaults to none.
	properties_exclude: Option<Vec<String>>,
	/// Area in square tile-pixels below which a polygon is dropped. Defaults to `4`.
	#[vpl(default = "4")]
	polygon_min_area: Option<f32>,
	/// Douglas-Peucker tolerance for polygons, in tile-pixels. Defaults to `4`.
	#[vpl(default = "4")]
	polygon_simplify: Option<f32>,
	/// Length in tile-pixels below which a line is dropped. Defaults to `4`.
	#[vpl(default = "4")]
	line_min_length: Option<f32>,
	/// Douglas-Peucker tolerance for lines, in tile-pixels. Defaults to `4`.
	#[vpl(default = "4")]
	line_simplify: Option<f32>,
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
	/// Whether to drop each feature's `id` before encoding. Defaults to `false`.
	#[vpl(default = "false")]
	ignore_id: Option<bool>,
}

/// Marker type for the read-factory macro. The actual runtime `TileSource`
/// is a [`FeatureTileSource`] returned from [`Operation::build`].
pub struct Operation;

impl Operation {
	#[context("Failed to build from_geo operation in VPL node {:?}", vpl_node.name)]
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

		let point_reduction = args.point_reduction;
		let compression = args.compression.unwrap_or(TileCompression::Gzip);
		let clip = args.bbox.map(BBoxClip::new).transpose()?;

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
		let mut features = load_features(&path, ext.as_str(), format_label, factory, clip.as_ref()).await?;
		if args.ignore_id.unwrap_or(false) {
			for f in &mut features {
				f.id = None;
			}
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
			&FeatureTileSourceArgs {
				layer_name: &layer_name,
				compression,
				label: "from_geo",
				source_description: "geo features",
				source_short: "geo",
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
	clip: Option<&BBoxClip>,
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
			drain(&s, clip).await?
		}
		"ndjson" | "ndgeojson" | "geojsonl" | "geojsonseq" => {
			let mut s = GeoJsonSource::new_line_delimited(path);
			if let Some(cb) = cb {
				s = s.with_progress(cb);
			}
			drain(&s, clip).await?
		}
		"shp" => {
			let mut s = ShapefileSource::new(path);
			if let Some(cb) = cb {
				s = s.with_progress(cb);
			}
			drain(&s, clip).await?
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

/// Drain a `FeatureSource`'s stream into a `Vec`, projecting each record to
/// web mercator and splitting `Multi*` geometries on the fly. This is what
/// [`FeatureImport::from_features`] expects as input — fusing the work into
/// the load loop avoids two extra full-Vec passes on what's typically a
/// multi-GB feature set.
///
/// A `bbox=` filters here rather than afterwards, for the same reason: on a
/// planet-scale extract cropped to one city, the features that are dropped are
/// nearly all of them, and never accumulating them is the whole point.
async fn drain<S: FeatureSource + ?Sized>(source: &S, clip: Option<&BBoxClip>) -> Result<Vec<GeoFeature>> {
	use versatiles_geometry::feature_import::project_and_flatten;
	let mut stream = source.load()?;
	let mut features = Vec::new();
	while let Some(item) = stream.next().await {
		features.extend(
			project_and_flatten(item?)?
				.into_iter()
				.filter(|f| clip.is_none_or(|c| c.keeps(f))),
		);
	}
	Ok(features)
}

crate::operations::macros::define_read_factory!("from_geo", Args, Operation);

#[cfg(test)]
mod tests {
	use versatiles_core::{TileCompression::Uncompressed, TileCoord};

	use super::*;

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
	async fn max_tile_bytes_propagates_to_single_tile_path() -> Result<()> {
		const COMMON: &str = "layer_name=\"places\" max_zoom=8 polygon_simplify=0 line_simplify=0 \
			 polygon_min_area=0 line_min_length=0";

		// A 1-byte cap makes the world tile over-cap: the single-tile API must
		// surface the error instead of silently returning no tile.
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_geo filename=\"../testdata/places.geojson\" max_tile_bytes=1 {COMMON}"
			))
			.await?;
		let err = op.tile(&TileCoord::new(0, 0, 0)?).await.unwrap_err();
		assert!(format!("{err:#}").contains("hard cap"), "{err:#}");

		// `max_tile_bytes=none` disables the hard cap: the same tile is emitted.
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_geo filename=\"../testdata/places.geojson\" max_tile_bytes=none {COMMON}"
			))
			.await?;
		assert!(
			op.tile(&TileCoord::new(0, 0, 0)?).await?.is_some(),
			"disabled hard cap must emit the world tile"
		);
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

	/// Input in the wrong CRS is refused where the mistake is, and says so.
	///
	/// Coordinates in metres used to be clamped to the corner of the world and
	/// then fail four layers down in `TileCover` with "x (180.0000000001) must be
	/// <= 180" — a message from which the actual mistake, data that was never
	/// reprojected, is not recoverable. A declared non-WGS84 `crs` member was not
	/// looked at at all.
	#[tokio::test]
	async fn input_in_the_wrong_crs_is_refused_by_name() -> Result<()> {
		let tmp = tempfile::tempdir()?;

		// EPSG:25832 easting/northing, no `crs` member to give it away.
		let projected = tmp.path().join("utm.geojson");
		std::fs::write(
			&projected,
			r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},
			   "geometry":{"type":"Point","coordinates":[389298.0,5819938.0]}}]}"#,
		)?;
		// Single quotes, not double: VPL processes escapes inside `"…"` and accepts
		// only `\\`, `\"`, `\n` and `\t`, so a Windows temp path — `C:\Users\…` —
		// fails to parse before the test reaches what it is about. A single-quoted
		// string is taken literally.
		let msg = format!(
			"{:#}",
			PipelineFactory::new_dummy()
				.operation_from_vpl(&format!("from_geo filename='{}'", projected.display()))
				.await
				.expect_err("projected coordinates should be refused")
		);
		assert!(msg.contains("389298"), "{msg}");
		assert!(msg.contains("EPSG:4326"), "{msg}");
		assert!(!msg.contains("TileCover"), "should fail before the tile cover: {msg}");

		// Lon/lat coordinates, but the file says they are something else.
		let declared = tmp.path().join("declared.geojson");
		std::fs::write(
			&declared,
			r#"{"type":"FeatureCollection",
			   "crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::25832"}},
			   "features":[{"type":"Feature","properties":{},
			   "geometry":{"type":"Point","coordinates":[13.405,52.52]}}]}"#,
		)?;
		let msg = format!(
			"{:#}",
			PipelineFactory::new_dummy()
				.operation_from_vpl(&format!("from_geo filename='{}'", declared.display()))
				.await
				.expect_err("a declared non-WGS84 CRS should be refused")
		);
		assert!(msg.contains("EPSG::25832"), "{msg}");
		assert!(msg.contains("only WGS84 lon/lat is supported"), "{msg}");
		Ok(())
	}

	/// Axis-swapped lat,lon input stays accepted, because it cannot be told apart.
	///
	/// Both numbers are in range, so nothing here can distinguish it from a place
	/// in the Indian Ocean. Pinned so that the range check is not later mistaken
	/// for protection against it — that half of the problem is a documentation
	/// matter, and the error message says so where a user will read it.
	#[tokio::test]
	async fn axis_swapped_input_is_not_detectable() -> Result<()> {
		let tmp = tempfile::tempdir()?;
		let swapped = tmp.path().join("swapped.geojson");
		std::fs::write(
			&swapped,
			r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},
			   "geometry":{"type":"Point","coordinates":[52.52,13.405]}}]}"#,
		)?;
		PipelineFactory::new_dummy()
			.operation_from_vpl(&format!("from_geo filename='{}' max_zoom=6", swapped.display()))
			.await?;
		Ok(())
	}

	/// `bbox=` bounds the output, not just the features that are read.
	///
	/// Dropping features alone would not do it: the fixture's polygons reach past
	/// any small box, so the data bounds — and with them the pyramid — would still
	/// cover everything the input touches. The pyramid has to be clipped too, or
	/// the operation would emit tiles the caller asked it not to.
	#[tokio::test]
	async fn bbox_bounds_the_pyramid_and_the_tiles() -> Result<()> {
		const FIXTURE: &str = "from_geo filename=\"../testdata/places.geojson\" max_zoom=8";

		let whole = PipelineFactory::new_dummy().operation_from_vpl(FIXTURE).await?;
		let cropped = PipelineFactory::new_dummy()
			.operation_from_vpl(&format!("{FIXTURE} bbox=[13.3,52.4,13.5,52.6]"))
			.await?;

		let bounds = |op: &dyn TileSource| -> (f64, f64, f64, f64) {
			op.metadata()
				.tile_pyramid()
				.expect("pyramid")
				.geo_bbox()
				.expect("bounds")
				.as_tuple()
		};
		let (w0, s0, e0, n0) = bounds(whole.as_ref());
		let (w1, s1, e1, n1) = bounds(cropped.as_ref());

		// The pyramid is a set of tiles, so the box it reports is the requested
		// one rounded out to tile edges: at max_zoom=8 a tile is 1.4° wide and the
		// whole request falls inside one of them. What must hold is that cropping
		// only ever shrinks, and shrinks something.
		assert!(
			w1 >= w0 && s1 >= s0 && e1 <= e0 && n1 <= n0,
			"{:?} vs {:?}",
			(w1, s1, e1, n1),
			(w0, s0, e0, n0)
		);
		assert!(
			e1 < e0,
			"the eastern arm of the fixture should have been cropped away: {e1} vs {e0}"
		);

		// Rounded out, but never by more than one tile at max_zoom.
		let tile_width = 360.0 / 2f64.powi(8);
		assert!(w1 > 13.3 - tile_width && e1 < 13.5 + tile_width, "{:?}", (w1, e1));

		// The single-tile path honours the same promise as the stream.
		let far_away = TileCoord::from_geo(0.0, 0.0, 8)?;
		assert!(whole.tile(&far_away).await?.is_none());
		assert!(cropped.tile(&far_away).await?.is_none());
		Ok(())
	}

	/// Features outside the box are dropped while reading, not carried and then
	/// clipped per tile — that is what makes cropping a planet-scale input
	/// affordable.
	#[tokio::test]
	async fn bbox_drops_features_outside_it() -> Result<()> {
		const FIXTURE: &str = "from_geo filename=\"../testdata/places.geojson\" layer_name=\"places\" \
			 max_zoom=8 polygon_simplify=0 line_simplify=0 polygon_min_area=0 line_min_length=0";

		// The fixture's `Districts` MultiPolygon has a piece around lon 14.0-14.5;
		// this box excludes it while keeping the three features over Berlin.
		let cropped = PipelineFactory::new_dummy()
			.operation_from_vpl(&format!("{FIXTURE} bbox=[12.9,52.4,13.9,52.8]"))
			.await?;

		let coord = TileCoord::from_geo(13.4, 52.5, 8)?;
		let tile = cropped.tile(&coord).await?.expect("tile over Berlin");
		let vt = versatiles_geometry::vector_tile::VectorTile::from_blob(&tile.into_blob(&Uncompressed)?)?;
		let names: Vec<String> = vt.layers[0]
			.features
			.iter()
			.filter_map(|f| f.to_feature(&vt.layers[0]).ok())
			.filter_map(|f| f.properties.get("name").map(ToString::to_string))
			.collect();
		assert!(!names.iter().any(|n| n.contains("Districts")), "{names:?}");
		Ok(())
	}

	/// A box that misses everything says so, instead of reporting that the bounds
	/// could not be computed — which reads like a broken file.
	#[tokio::test]
	async fn a_bbox_that_selects_nothing_says_so() {
		let msg = format!(
			"{:#}",
			PipelineFactory::new_dummy()
				.operation_from_vpl("from_geo filename=\"../testdata/places.geojson\" bbox=[0,0,1,1]")
				.await
				.expect_err("a box over the Atlantic should be refused")
		);
		assert!(msg.contains("selects no features"), "{msg}");
		assert!(!msg.contains("failed to compute bounds"), "{msg}");
	}

	#[tokio::test]
	async fn unsupported_args_error() -> Result<()> {
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
