//! In-memory feature-import engine.
//!
//! Takes a `Vec<GeoFeature>` (loaded from one of the [`crate::feature_source`]
//! adapters), projects every geometry to web mercator, and prepares per-zoom
//! feature lists with a spatial index. [`FeatureImport::get_tile`] then
//! renders any tile lazily by querying the index, clipping, quantizing, and
//! encoding MVT.
//!
//! ## Pipeline (top-down cascade)
//!
//! 1. Build an [`crate::arc_graph::ArcGraph`] once from the projected,
//!    multi-geometry-flattened features. Shared polygon borders and shared
//!    line segments collapse into a single arc.
//! 2. Iterate zoom levels from `max_zoom` down to `min_zoom`. At each step,
//!    simplify the *previous step's* arcs at the current zoom's tolerance
//!    (Douglas-Peucker is monotonic, so chaining is identical to simplifying
//!    the original from scratch — a strict speed-up since work shrinks as
//!    we go down); reassemble only the features that survived the previous
//!    zoom's filters; drop polygons by min-area and lines by min-length at
//!    this zoom's thresholds; apply the `point_reduction` strategy on top of
//!    the previous zoom's surviving point set (cumulative across zooms — a
//!    point dropped at a finer zoom can never reappear at a coarser one);
//!    and build an R-tree over the survivors for fast tile-bbox queries.

mod heuristics;
mod reduce_lines;
mod reduce_points;
mod reduce_polygons;
mod spatial_index;
mod tile_render;

pub use heuristics::auto_max_zoom;
pub use reduce_points::PointReductionStrategy;
pub use tile_render::{clip_geometry, render_tile};
use versatiles_derive::context;

use crate::arc_graph::{self, ArcGraph, FeatureArcs};
use crate::ext::{MercatorExt, coord_from_mercator};
use crate::geo::GeoFeature;
use crate::vector_tile::VectorTile;
use anyhow::{Result, bail};
use geo::BoundingRect;
use geo_types::{Coord, Geometry};
use heuristics::auto_max_zoom_projected;
use rstar::RTree;
use spatial_index::{FeatureRef, query};
use versatiles_core::{GeoBBox, WORLD_SIZE};

/// MVT tile-local coordinate extent (4096 pixels per tile, the conventional default).
pub const TILE_EXTENT: u32 = 4096;

/// Configuration for [`FeatureImport`].
#[derive(Clone, Debug)]
pub struct FeatureImportConfig {
	pub layer_name: String,
	pub min_zoom: u8,
	/// Highest zoom level emitted. `None` triggers the auto-heuristic
	/// (median feature size ≈ 4 tile-pixels, capped at 14).
	pub max_zoom: Option<u8>,
	/// Douglas-Peucker tolerance for polygons, in tile-pixels at the current zoom.
	pub polygon_simplify_px: f32,
	/// Douglas-Peucker tolerance for lines, in tile-pixels at the current zoom.
	pub line_simplify_px: f32,
	/// Drop polygons whose area at the current zoom is below this many tile-pixels².
	/// `0.0` disables the filter.
	pub polygon_min_area_px: f32,
	/// Drop lines whose length at the current zoom is below this many tile-pixels.
	/// `0.0` disables the filter.
	pub line_min_length_px: f32,
	/// Point-reduction strategy applied per-zoom; cumulative across zooms (a
	/// point dropped at zoom z+1 cannot reappear at zoom z). See
	/// [`PointReductionStrategy`].
	pub point_reduction: PointReductionStrategy,
	/// Numeric value whose meaning depends on `point_reduction`:
	/// - `DropRate`: per-zoom keep-fraction (in `[0, 1]`). Composes
	///   geometrically across zooms — at `max_zoom - k`, the cumulative
	///   keep-ratio is `value^k`.
	/// - `MinDistance`: minimum distance between kept points, in tile-pixels
	///   *at the current zoom*. Equivalent to a coarser threshold (in meters)
	///   at lower zooms.
	pub point_reduction_value: f32,
}

impl Default for FeatureImportConfig {
	fn default() -> Self {
		Self {
			layer_name: "features".to_string(),
			min_zoom: 0,
			max_zoom: None, // auto via `auto_max_zoom`
			polygon_simplify_px: 4.0,
			line_simplify_px: 4.0,
			polygon_min_area_px: 4.0,
			line_min_length_px: 4.0,
			point_reduction: PointReductionStrategy::None,
			point_reduction_value: 0.0,
		}
	}
}

#[derive(Debug)]
struct ZoomLayer {
	features: Vec<GeoFeature>,
	rtree: RTree<FeatureRef>,
}

/// In-memory import: features projected to mercator, simplified per zoom,
/// indexed for tile-bbox queries.
#[derive(Debug)]
pub struct FeatureImport {
	config: FeatureImportConfig,
	/// User intent → resolved at construction. Reflects the auto-heuristic
	/// when `config.max_zoom` was `None`.
	resolved_max_zoom: u8,
	/// Indexed by zoom level. `None` for zooms outside `[min_zoom, max_zoom]`.
	layers: Vec<Option<ZoomLayer>>,
	bounds_mercator: [f64; 4],
	/// Property name → MVT/TileJSON field type ("Boolean" / "Number" / "String"),
	/// computed from the input features before cascading. Drives the TileJSON
	/// `vector_layers` entry that consumers like QGIS need to discover layers.
	property_schema: std::collections::BTreeMap<String, String>,
}

impl FeatureImport {
	/// Build the import from a vector of features.
	///
	/// Callers typically drain a [`crate::feature_source::FeatureSource`]'s
	/// stream into a `Vec<GeoFeature>` first, then pass it here.
	#[context("importing features")]
	pub fn from_features(features: Vec<GeoFeature>, config: FeatureImportConfig) -> Result<Self> {
		// Project to web mercator (only once — the auto-`max_zoom` heuristic
		// reuses these projected geometries instead of re-projecting).
		// TODO: validate CRS once the GeoJSON parser tracks it. v1 trusts the
		// caller to pass WGS84 lon/lat; non-WGS84 input silently produces
		// garbage mercator coordinates.
		log::debug!("projecting {} features to mercator", features.len());
		// Snapshot the property schema before we consume the features. Drives the
		// TileJSON `vector_layers` entry; sticking to a single TileJSON-spec
		// type per field, picking the most informative on collisions
		// (Boolean < Number < String).
		let property_schema = collect_property_schema(&features);
		let projected: Vec<GeoFeature> = features
			.into_iter()
			.map(|mut f| {
				f.geometry = f.geometry.to_mercator();
				f
			})
			.collect();

		// Resolve the auto-`max_zoom` heuristic against the projected features
		// so we don't pay for projection twice.
		let resolved_max_zoom = config.max_zoom.unwrap_or_else(|| auto_max_zoom_projected(&projected));
		if config.min_zoom > resolved_max_zoom {
			bail!("min_zoom ({}) > max_zoom ({resolved_max_zoom})", config.min_zoom);
		}

		// Flatten Multi* into N independent features.
		log::debug!("flattening {} features", projected.len());
		let flattened: Vec<GeoFeature> = projected.into_iter().flat_map(flatten_feature).collect();
		let bounds_mercator = features_bbox(&flattened).ok_or_else(|| anyhow::anyhow!("failed to compute bounds"))?;

		// Build the arc graph once. Per-zoom simplification simplifies each
		// arc and reassembles features so shared boundaries stay aligned.
		let (arc_graph, feature_arcs): (ArcGraph, Vec<FeatureArcs>) = arc_graph::build(&flattened);

		// Top-down cascade: simplify arcs and prune features once at max_zoom,
		// then carry the result down to each lower zoom. See the module
		// docstring for the rationale.
		let n_slots = usize::from(resolved_max_zoom) + 1;
		let mut layers: Vec<Option<ZoomLayer>> = (0..n_slots).map(|_| None).collect();
		log::debug!(
			"building zoom layers for zooms {}..={resolved_max_zoom} (cascading max→min)",
			config.min_zoom,
		);

		// Cascade state. `arcs` shrinks as we simplify further at each step.
		// `alive_indices` are the indices into `flattened` of features still
		// passing every filter applied so far (max_zoom..z+1).
		let mut arcs: Vec<arc_graph::Arc> = arc_graph.arcs().to_vec();
		let mut alive_indices: Vec<usize> = (0..flattened.len()).collect();

		for z in (config.min_zoom..=resolved_max_zoom).rev() {
			log::trace!("processing zoom {z}");
			let m_per_px = meters_per_pixel(z);
			let tol_simplify_m = arc_simplify_tolerance(&config, m_per_px);
			let polygon_min_area_m2 = f64::from(config.polygon_min_area_px) * m_per_px * m_per_px;
			let line_min_length_m = f64::from(config.line_min_length_px) * m_per_px;

			// Cascade step on the arcs (DP is monotonic across tolerances).
			arcs = arc_graph::simplify_arcs(&arcs, tol_simplify_m);

			// Reassemble only the still-alive features at this zoom.
			let reassembled: Vec<(usize, GeoFeature)> = alive_indices
				.iter()
				.map(|&i| {
					let template = &flattened[i];
					let geometry = arc_graph::reassemble_geometry(&arcs, &feature_arcs[i]);
					(
						i,
						GeoFeature {
							id: template.id.clone(),
							geometry,
							properties: template.properties.clone(),
						},
					)
				})
				.collect();

			// Re-apply min-area / min-length filters at this zoom's (larger)
			// thresholds. Both filters are monotonic in the threshold, so this
			// can only shrink `alive_indices`.
			let filtered: Vec<(usize, GeoFeature)> = reassembled
				.into_iter()
				.filter(|(_, f)| reduce_polygons::passes_min_area(&f.geometry, polygon_min_area_m2))
				.filter(|(_, f)| reduce_lines::passes_min_length(&f.geometry, line_min_length_m))
				.collect();

			// Cascade point reduction: each strategy operates on the previous
			// zoom's survivors so reductions chain naturally.
			//
			// - `DropRate(v)` drops `1 - v` of the *current* survivors. Across
			//   k zooms this composes to a `v^k` keep-ratio — same shape as
			//   the prior per-zoom `value^(max_zoom - z)`.
			// - `MinDistance(d * m_per_px)` drops points closer than `d`
			//   tile-pixels at this zoom; survivors at coarser zooms are a
			//   subset of those at finer zooms.
			let reduced = match config.point_reduction {
				PointReductionStrategy::None => filtered,
				PointReductionStrategy::DropRate => {
					let keep_ratio = f64::from(config.point_reduction_value);
					reduce_points::apply_drop_rate(filtered, keep_ratio)
				}
				PointReductionStrategy::MinDistance => {
					let threshold_m = f64::from(config.point_reduction_value) * m_per_px;
					reduce_points::apply_min_distance(filtered, threshold_m)
				}
			};

			// Carry surviving original indices forward to the next coarser zoom.
			alive_indices = reduced.iter().map(|(i, _)| *i).collect();

			// Drop features without a bounding rect so the R-tree and the
			// feature list stay aligned (orphans wouldn't be retrievable).
			let zoom_features: Vec<GeoFeature> = reduced
				.into_iter()
				.map(|(_, f)| f)
				.filter(|f| f.geometry.bounding_rect().is_some())
				.collect();

			let rtree = build_rtree(&zoom_features);
			layers[usize::from(z)] = Some(ZoomLayer {
				features: zoom_features,
				rtree,
			});
		}

		Ok(Self {
			config,
			resolved_max_zoom,
			layers,
			bounds_mercator,
			property_schema,
		})
	}

	/// Property-name → TileJSON field-type map ("Boolean" / "Number" / "String"),
	/// derived from the input features. Use this to populate the TileJSON
	/// `vector_layers` entry so MBTiles consumers (e.g. QGIS) can discover
	/// what's in each layer.
	#[must_use]
	pub fn property_schema(&self) -> &std::collections::BTreeMap<String, String> {
		&self.property_schema
	}

	/// Render the MVT tile at `(z, x, y)`. Returns `Ok(None)` for zooms
	/// outside `[min_zoom, max_zoom]` and tiles with no surviving features.
	pub fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<VectorTile>> {
		let Some(layer) = self.layers.get(usize::from(z)).and_then(Option::as_ref) else {
			return Ok(None);
		};
		let tile_bbox = tile_mercator_bbox(z, x, y);
		let candidates: Vec<&FeatureRef> = query(&layer.rtree, tile_bbox).collect();
		if candidates.is_empty() {
			return Ok(None);
		}
		let candidate_features: Vec<GeoFeature> = candidates
			.into_iter()
			.map(|r| layer.features[r.index].clone())
			.collect();
		render_tile(candidate_features, &self.config.layer_name, tile_bbox, TILE_EXTENT)
	}

	/// The mercator bbox of all input features, or `None` if the input was empty.
	#[must_use]
	pub fn bounds_mercator(&self) -> [f64; 4] {
		self.bounds_mercator
	}

	/// The data bbox in WGS84 (lon/lat degrees), or `None` if the input was empty.
	pub fn bounds_geo(&self) -> Result<Option<GeoBBox>> {
		let [xmin, ymin, xmax, ymax] = self.bounds_mercator;
		let min = coord_from_mercator(Coord { x: xmin, y: ymin });
		let max = coord_from_mercator(Coord { x: xmax, y: ymax });
		Ok(Some(GeoBBox::new(min.x, min.y, max.x, max.y)?))
	}

	#[must_use]
	pub fn config(&self) -> &FeatureImportConfig {
		&self.config
	}

	/// The effective `max_zoom`: either `config.max_zoom` if set, or the
	/// auto-heuristic value computed during construction.
	#[must_use]
	pub fn max_zoom(&self) -> u8 {
		self.resolved_max_zoom
	}

	/// The effective `min_zoom` (just a passthrough; included for symmetry).
	#[must_use]
	pub fn min_zoom(&self) -> u8 {
		self.config.min_zoom
	}
}

fn tile_mercator_bbox(z: u8, x: u32, y: u32) -> [f64; 4] {
	let tiles_per_side = f64::from(2u32.pow(u32::from(z)));
	let tile_size = WORLD_SIZE / tiles_per_side;
	let xmin = -WORLD_SIZE / 2.0 + f64::from(x) * tile_size;
	let xmax = xmin + tile_size;
	let ymax = WORLD_SIZE / 2.0 - f64::from(y) * tile_size;
	let ymin = ymax - tile_size;
	[xmin, ymin, xmax, ymax]
}

fn meters_per_pixel(zoom: u8) -> f64 {
	let tiles_per_side = f64::from(2u32.pow(u32::from(zoom)));
	let tile_size_m = WORLD_SIZE / tiles_per_side;
	tile_size_m / f64::from(TILE_EXTENT)
}

fn flatten_feature(feature: GeoFeature) -> Vec<GeoFeature> {
	let is_multi = matches!(
		feature.geometry,
		Geometry::MultiPoint(_) | Geometry::MultiLineString(_) | Geometry::MultiPolygon(_)
	);
	if !is_multi {
		return vec![feature];
	}
	let GeoFeature {
		id,
		geometry,
		properties,
	} = feature;
	match geometry {
		Geometry::MultiPoint(mp) => mp
			.0
			.into_iter()
			.map(|p| GeoFeature {
				id: id.clone(),
				geometry: Geometry::Point(p),
				properties: properties.clone(),
			})
			.collect(),
		Geometry::MultiLineString(ml) => ml
			.0
			.into_iter()
			.map(|ls| GeoFeature {
				id: id.clone(),
				geometry: Geometry::LineString(ls),
				properties: properties.clone(),
			})
			.collect(),
		Geometry::MultiPolygon(mp) => mp
			.0
			.into_iter()
			.map(|p| GeoFeature {
				id: id.clone(),
				geometry: Geometry::Polygon(p),
				properties: properties.clone(),
			})
			.collect(),
		_ => unreachable!("checked is_multi above"),
	}
}

/// Combined simplification tolerance for the arc graph at the given pixels-per-meter.
///
/// The arc graph stores one arc per shared edge — an arc traversed by both
/// polygons and lines must use a single tolerance. v1 picks the *minimum*
/// non-zero of `polygon_simplify_px` and `line_simplify_px` as a conservative
/// shared tolerance.
fn arc_simplify_tolerance(config: &FeatureImportConfig, m_per_px: f64) -> f64 {
	let p = f64::from(config.polygon_simplify_px);
	let l = f64::from(config.line_simplify_px);
	let combined_px = match (p > 0.0, l > 0.0) {
		(true, true) => p.min(l),
		(true, false) => p,
		(false, true) => l,
		(false, false) => 0.0,
	};
	combined_px * m_per_px
}

/// Walk every feature's properties and accumulate `name → TileJSON-field-type`.
///
/// TileJSON 3.0 §3.3.2 only allows three field types: `Boolean`, `Number`, and
/// `String`. We map [`crate::geo::GeoValue`] variants accordingly, ignoring
/// `Null` (no type signal). On a name collision between Boolean/Number/String
/// we promote to the most permissive ("String" wins, then "Number", then
/// "Boolean") so the schema covers every value the consumer might see.
fn collect_property_schema(features: &[GeoFeature]) -> std::collections::BTreeMap<String, String> {
	use crate::geo::GeoValue;
	fn rank(t: &str) -> u8 {
		match t {
			"Boolean" => 1,
			"Number" => 2,
			"String" => 3,
			_ => 0,
		}
	}
	let mut schema: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
	for feature in features {
		for (name, value) in feature.properties.iter() {
			let new_type = match value {
				GeoValue::Bool(_) => "Boolean",
				GeoValue::Int(_) | GeoValue::UInt(_) | GeoValue::Float(_) | GeoValue::Double(_) => "Number",
				GeoValue::String(_) => "String",
				GeoValue::Null => continue,
			};
			schema
				.entry(name.clone())
				.and_modify(|existing| {
					if rank(new_type) > rank(existing) {
						*existing = new_type.to_string();
					}
				})
				.or_insert_with(|| new_type.to_string());
		}
	}
	schema
}

fn features_bbox(features: &[GeoFeature]) -> Option<[f64; 4]> {
	let mut acc: Option<(f64, f64, f64, f64)> = None;
	for f in features {
		if let Some(rect) = f.geometry.bounding_rect() {
			let (xmin, ymin, xmax, ymax) = (rect.min().x, rect.min().y, rect.max().x, rect.max().y);
			acc = Some(match acc {
				None => (xmin, ymin, xmax, ymax),
				Some((a, b, c, d)) => (a.min(xmin), b.min(ymin), c.max(xmax), d.max(ymax)),
			});
		}
	}
	acc.map(|(a, b, c, d)| [a, b, c, d])
}

fn build_rtree(features: &[GeoFeature]) -> RTree<FeatureRef> {
	let refs: Vec<FeatureRef> = features
		.iter()
		.enumerate()
		.filter_map(|(i, f)| {
			f.geometry
				.bounding_rect()
				.map(|r| FeatureRef::new(i, [r.min().x, r.min().y, r.max().x, r.max().y]))
		})
		.collect();
	RTree::bulk_load(refs)
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::geo::GeoValue;
	use geo_types::{LineString, Point, Polygon};

	fn point_feature(id: u64, name: &str, lon: f64, lat: f64) -> GeoFeature {
		let mut f = GeoFeature::new(Geometry::Point(Point::new(lon, lat)));
		f.set_property("name".into(), name);
		f.set_id(GeoValue::from(id));
		f
	}

	#[test]
	fn imports_two_points_and_renders_world_tile() -> Result<()> {
		let features = vec![
			point_feature(1, "origin", 0.0, 0.0),
			point_feature(2, "east", 90.0, 30.0),
		];
		let config = FeatureImportConfig {
			max_zoom: Some(5),
			..Default::default()
		};
		let import = FeatureImport::from_features(features, config)?;

		assert_eq!(import.bounds_mercator().map(|b| b as i64), [0, 0, 10018754, 3503549]);

		// Tile (0, 0, 0) covers the whole world; both points must appear.
		let tile = import.get_tile(0, 0, 0)?.expect("world tile is non-empty");
		assert_eq!(tile.layers.len(), 1);
		assert_eq!(tile.layers[0].name, "features");
		assert_eq!(tile.layers[0].features.len(), 2);
		Ok(())
	}

	#[test]
	fn empty_input_yields_no_tiles() -> Result<()> {
		let import = FeatureImport::from_features(Vec::new(), FeatureImportConfig::default());
		assert_eq!(import.unwrap_err().to_string(), "importing features");
		Ok(())
	}

	#[test]
	fn out_of_range_zoom_returns_none() -> Result<()> {
		let config = FeatureImportConfig {
			max_zoom: Some(3),
			..Default::default()
		};
		let import = FeatureImport::from_features(vec![point_feature(1, "o", 0.0, 0.0)], config)?;
		assert!(import.get_tile(10, 0, 0)?.is_none());
		Ok(())
	}

	#[test]
	fn drops_tiny_polygon_at_low_zoom() -> Result<()> {
		// A tiny polygon (~1m × 1m) is below `polygon_min_area_px=4` at *every*
		// zoom (it's never larger than 4 px²), so no tile should contain it.
		let exterior = LineString::from(vec![
			[13.40500, 52.52000],
			[13.40501, 52.52000],
			[13.40501, 52.52001],
			[13.40500, 52.52001],
			[13.40500, 52.52000],
		]);
		let polygon = Polygon::new(exterior, vec![]);
		let feature = GeoFeature::new(Geometry::Polygon(polygon));

		let config = FeatureImportConfig {
			max_zoom: Some(5),
			polygon_simplify_px: 0.0,
			..Default::default()
		};
		let import = FeatureImport::from_features(vec![feature], config)?;
		// Tile (z=5) over Berlin is the smallest tile we built.
		let coord = versatiles_core::TileCoord::from_geo(13.405, 52.52, 5)?;
		assert!(import.get_tile(coord.level, coord.x, coord.y)?.is_none());
		Ok(())
	}

	#[test]
	fn drops_short_line_at_low_zoom() -> Result<()> {
		// A ~10m line at high zoom is fine; at low zoom (z=0, 1 px ≈ 9.8 km),
		// it's far below `line_min_length_px=4` (which means ≥ 39 km at z=0).
		let line = LineString::from(vec![[13.405, 52.520], [13.406, 52.520]]);
		let feature = GeoFeature::new(Geometry::LineString(line));
		let config = FeatureImportConfig {
			max_zoom: Some(14),
			line_simplify_px: 0.0,
			..Default::default()
		};
		let import = FeatureImport::from_features(vec![feature], config)?;
		// At z=0, the line is too short.
		assert!(import.get_tile(0, 0, 0)?.is_none());
		// At z=14, the line is large enough.
		let coord = versatiles_core::TileCoord::from_geo(13.405, 52.52, 14)?;
		assert!(import.get_tile(coord.level, coord.x, coord.y)?.is_some());
		Ok(())
	}

	#[test]
	fn polygon_clipped_to_tile() -> Result<()> {
		// A polygon that covers most of the world; it should appear in many tiles
		// but be clipped down to each tile's extent.
		let exterior = LineString::from(vec![
			[-90.0, -45.0],
			[90.0, -45.0],
			[90.0, 45.0],
			[-90.0, 45.0],
			[-90.0, -45.0],
		]);
		let polygon = Polygon::new(exterior, vec![]);
		let mut feature = GeoFeature::new(Geometry::Polygon(polygon));
		feature.set_property("kind".into(), "boundary");

		let config = FeatureImportConfig {
			max_zoom: Some(3),
			polygon_simplify_px: 0.0, // disable simplification for this test
			..Default::default()
		};
		let import = FeatureImport::from_features(vec![feature], config)?;

		let tile = import.get_tile(2, 1, 1)?.expect("tile in the polygon");
		assert_eq!(tile.layers.len(), 1);
		assert_eq!(tile.layers[0].features.len(), 1);
		Ok(())
	}

	#[test]
	fn auto_max_zoom_for_country_scale_polygon() {
		// A 10°×10° polygon ≈ 1100 km in mercator. At z=0 it's already ~112 px
		// wide — way above the 4-px target. log2(4·40075km/(1100km·4096)) is
		// negative, so the heuristic clamps to 0. (Interpretation: the data is
		// huge enough at z=0 that no extra detail is needed.)
		let exterior = LineString::from(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]);
		let f = GeoFeature::new(Geometry::Polygon(Polygon::new(exterior, vec![])));
		assert_eq!(auto_max_zoom(std::slice::from_ref(&f)), 0);
	}

	#[test]
	fn auto_max_zoom_for_kilometer_scale_polygon() {
		// A ~1 km polygon at the equator: 0.009°×0.009° lon/lat ≈ 1 km × 1 km
		// in mercator. log2(4·40075/(1·4096)) ≈ log2(39) ≈ 5.3 → 5.
		let exterior = LineString::from(vec![[0.0, 0.0], [0.009, 0.0], [0.009, 0.009], [0.0, 0.009], [0.0, 0.0]]);
		let f = GeoFeature::new(Geometry::Polygon(Polygon::new(exterior, vec![])));
		let z = auto_max_zoom(std::slice::from_ref(&f));
		assert!((4..=6).contains(&z), "expected ~5, got {z}");
	}

	#[test]
	fn auto_max_zoom_for_meter_scale_features_caps_at_14() {
		// A 1 m × 1 m polygon would suggest zoom > 14; should cap at 14.
		let exterior = LineString::from(vec![
			[0.000_001, 0.000_001],
			[0.000_010, 0.000_001],
			[0.000_010, 0.000_010],
			[0.000_001, 0.000_010],
			[0.000_001, 0.000_001],
		]);
		let f = GeoFeature::new(Geometry::Polygon(Polygon::new(exterior, vec![])));
		assert_eq!(auto_max_zoom(std::slice::from_ref(&f)), 14);
	}

	#[test]
	fn auto_max_zoom_for_point_only_input_defaults_to_14() {
		let p1 = GeoFeature::new(Geometry::Point(Point::new(0.0, 0.0)));
		let p2 = GeoFeature::new(Geometry::Point(Point::new(1.0, 1.0)));
		assert_eq!(auto_max_zoom(&[p1, p2]), 14);
	}

	#[test]
	fn auto_max_zoom_uses_median_not_mean() {
		// One enormous polygon (continent-scale) and many tiny polygons. The
		// median is small → high zoom suggested.
		let huge = LineString::from(vec![
			[-90.0, -45.0],
			[90.0, -45.0],
			[90.0, 45.0],
			[-90.0, 45.0],
			[-90.0, -45.0],
		]);
		let mut features: Vec<GeoFeature> = Vec::new();
		features.push(GeoFeature::new(Geometry::Polygon(Polygon::new(huge, vec![]))));
		// 10 small polygons (~10 m × 10 m each).
		for i in 0..10 {
			let off = f64::from(i) * 0.001;
			let small = LineString::from(vec![
				[off, off],
				[off + 0.0001, off],
				[off + 0.0001, off + 0.0001],
				[off, off + 0.0001],
				[off, off],
			]);
			features.push(GeoFeature::new(Geometry::Polygon(Polygon::new(small, vec![]))));
		}
		let z = auto_max_zoom(&features);
		// Median feature size is small → should return 14 or near it.
		assert!(z >= 12, "expected ≥12 (median is small), got {z}");
	}

	#[tokio::test]
	async fn from_features_via_geojson_source() -> Result<()> {
		use crate::feature_source::{FeatureSource, GeoJsonSource};
		use futures::StreamExt;
		let src = GeoJsonSource::new("../testdata/places.geojson");
		// Disable simplification + reduction so this test exercises only the
		// import + render path. (At z=0, the default simplify tolerance is
		// kilometers-large, which is intentional but would collapse the
		// fixture's small polygons into degenerate triangles.)
		let config = FeatureImportConfig {
			layer_name: "places".to_string(),
			max_zoom: Some(5),
			polygon_simplify_px: 0.0,
			line_simplify_px: 0.0,
			polygon_min_area_px: 0.0,
			line_min_length_px: 0.0,
			..Default::default()
		};
		let mut stream = src.load()?;
		let mut features = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}
		let import = FeatureImport::from_features(features, config)?;
		assert_eq!(
			import.bounds_mercator().map(|b| b as i64),
			[1447153, 6800125, 1614132, 6927697]
		);

		let tile = import.get_tile(0, 0, 0)?.expect("world tile non-empty");
		assert_eq!(tile.layers[0].name, "places");
		// 4 input features but the MultiPolygon flattens to 2, total 5.
		assert_eq!(tile.layers[0].features.len(), 5);
		Ok(())
	}
}
