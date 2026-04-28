//! In-memory feature-import engine.
//!
//! Drains a [`crate::feature_source::FeatureSource`] stream, projects every
//! geometry to web mercator, and prepares per-zoom feature lists with a
//! spatial index. [`FeatureImport::get_tile`] then renders any tile lazily
//! by querying the index, clipping, quantizing, and encoding MVT.
//!
//! Phase 1 uses [`geo::Simplify`] (Douglas-Peucker) per feature for
//! simplification. Phase 5 will replace this with a topology-preserving
//! arc-graph implementation behind the same configuration knobs.

mod reduce_lines;
mod reduce_points;
mod reduce_polygons;
mod spatial_index;
mod tile_render;

pub use reduce_points::PointReductionStrategy;
pub use tile_render::{clip_geometry, render_tile};

use crate::ext::{MercatorExt, coord_from_mercator};
use crate::feature_source::FeatureSource;
use crate::geo::GeoFeature;
use crate::vector_tile::VectorTile;
use anyhow::{Result, bail};
use futures::StreamExt;
use geo::{BoundingRect, Simplify};
use geo_types::{Coord, Geometry};
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
	pub max_zoom: u8,
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
	/// Point-reduction strategy applied per-zoom. See [`PointReductionStrategy`].
	pub point_reduction: PointReductionStrategy,
	/// Numeric value whose meaning depends on `point_reduction`:
	/// - `DropRate`: keep-fraction per zoom step (in `[0, 1]`).
	/// - `MinDistance`: minimum distance between kept points, in tile-pixels.
	pub point_reduction_value: f32,
}

impl Default for FeatureImportConfig {
	fn default() -> Self {
		Self {
			layer_name: "features".to_string(),
			min_zoom: 0,
			max_zoom: 14,
			polygon_simplify_px: 4.0,
			line_simplify_px: 4.0,
			polygon_min_area_px: 4.0,
			line_min_length_px: 4.0,
			point_reduction: PointReductionStrategy::None,
			point_reduction_value: 0.0,
		}
	}
}

struct ZoomLayer {
	features: Vec<GeoFeature>,
	rtree: RTree<FeatureRef>,
}

/// In-memory import: features projected to mercator, simplified per zoom,
/// indexed for tile-bbox queries.
pub struct FeatureImport {
	config: FeatureImportConfig,
	/// Indexed by zoom level. `None` for zooms outside `[min_zoom, max_zoom]`.
	layers: Vec<Option<ZoomLayer>>,
	bounds_mercator: Option<[f64; 4]>,
}

impl FeatureImport {
	/// Drain a [`FeatureSource`]'s stream and build the import.
	pub async fn from_source<S: FeatureSource + ?Sized>(source: &S, config: FeatureImportConfig) -> Result<Self> {
		let mut stream = source.load()?;
		let mut features = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}
		Self::from_features(features, config)
	}

	/// Build the import directly from a vector of features (synchronous path
	/// for tests and callers that already have features in memory).
	pub fn from_features(features: Vec<GeoFeature>, config: FeatureImportConfig) -> Result<Self> {
		if config.min_zoom > config.max_zoom {
			bail!("min_zoom ({}) > max_zoom ({})", config.min_zoom, config.max_zoom);
		}

		// Project to web mercator.
		let projected: Vec<GeoFeature> = features
			.into_iter()
			.map(|mut f| {
				f.geometry = f.geometry.to_mercator();
				f
			})
			.collect();

		// Flatten Multi* into N independent features.
		let flattened: Vec<GeoFeature> = projected.into_iter().flat_map(flatten_feature).collect();
		let bounds_mercator = features_bbox(&flattened);

		// Per-zoom simplification + reduction + spatial index.
		let n_slots = usize::from(config.max_zoom) + 1;
		let mut layers: Vec<Option<ZoomLayer>> = (0..n_slots).map(|_| None).collect();
		for z in config.min_zoom..=config.max_zoom {
			let m_per_px = meters_per_pixel(z);
			let tol_polygon_m = f64::from(config.polygon_simplify_px) * m_per_px;
			let tol_line_m = f64::from(config.line_simplify_px) * m_per_px;
			let polygon_min_area_m2 = f64::from(config.polygon_min_area_px) * m_per_px * m_per_px;
			let line_min_length_m = f64::from(config.line_min_length_px) * m_per_px;

			// Carry the original `flattened` index along so point-reduction
			// strategies hash on a stable identifier.
			let indexed: Vec<(usize, GeoFeature)> = flattened
				.iter()
				.enumerate()
				.map(|(idx, f)| (idx, simplify_feature(f, tol_polygon_m, tol_line_m)))
				.filter(|(_, f)| reduce_polygons::passes_min_area(&f.geometry, polygon_min_area_m2))
				.filter(|(_, f)| reduce_lines::passes_min_length(&f.geometry, line_min_length_m))
				.collect();
			let reduced = match config.point_reduction {
				PointReductionStrategy::None => indexed,
				PointReductionStrategy::DropRate => {
					// Cumulative keep ratio: `value^(max_zoom - z)`. At max_zoom it's 1.0.
					let exp = i32::from(config.max_zoom.saturating_sub(z));
					let keep_ratio = f64::from(config.point_reduction_value).powi(exp);
					reduce_points::apply_drop_rate(indexed, keep_ratio)
				}
				PointReductionStrategy::MinDistance => {
					let threshold_m = f64::from(config.point_reduction_value) * m_per_px;
					reduce_points::apply_min_distance(indexed, threshold_m)
				}
			};
			let zoom_features: Vec<GeoFeature> = reduced.into_iter().map(|(_, f)| f).collect();

			let rtree = build_rtree(&zoom_features);
			layers[usize::from(z)] = Some(ZoomLayer {
				features: zoom_features,
				rtree,
			});
		}

		Ok(Self {
			config,
			layers,
			bounds_mercator,
		})
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
	pub fn bounds_mercator(&self) -> Option<[f64; 4]> {
		self.bounds_mercator
	}

	/// The data bbox in WGS84 (lon/lat degrees), or `None` if the input was empty.
	pub fn bounds_geo(&self) -> Result<Option<GeoBBox>> {
		let Some([xmin, ymin, xmax, ymax]) = self.bounds_mercator else {
			return Ok(None);
		};
		let min = coord_from_mercator(Coord { x: xmin, y: ymin });
		let max = coord_from_mercator(Coord { x: xmax, y: ymax });
		Ok(Some(GeoBBox::new(min.x, min.y, max.x, max.y)?))
	}

	#[must_use]
	pub fn config(&self) -> &FeatureImportConfig {
		&self.config
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

fn simplify_feature(feature: &GeoFeature, tol_polygon_m: f64, tol_line_m: f64) -> GeoFeature {
	let geometry = match &feature.geometry {
		Geometry::LineString(ls) if tol_line_m > 0.0 => Geometry::LineString(ls.simplify(tol_line_m)),
		Geometry::Polygon(p) if tol_polygon_m > 0.0 => Geometry::Polygon(p.simplify(tol_polygon_m)),
		other => other.clone(),
	};
	GeoFeature {
		id: feature.id.clone(),
		geometry,
		properties: feature.properties.clone(),
	}
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
			max_zoom: 5,
			..Default::default()
		};
		let import = FeatureImport::from_features(features, config)?;

		assert!(import.bounds_mercator().is_some());

		// Tile (0, 0, 0) covers the whole world; both points must appear.
		let tile = import.get_tile(0, 0, 0)?.expect("world tile is non-empty");
		assert_eq!(tile.layers.len(), 1);
		assert_eq!(tile.layers[0].name, "features");
		assert_eq!(tile.layers[0].features.len(), 2);
		Ok(())
	}

	#[test]
	fn empty_input_yields_no_tiles() -> Result<()> {
		let import = FeatureImport::from_features(Vec::new(), FeatureImportConfig::default())?;
		assert!(import.bounds_mercator().is_none());
		assert!(import.get_tile(0, 0, 0)?.is_none());
		Ok(())
	}

	#[test]
	fn out_of_range_zoom_returns_none() -> Result<()> {
		let config = FeatureImportConfig {
			max_zoom: 3,
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
			max_zoom: 5,
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
			max_zoom: 14,
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
			max_zoom: 3,
			polygon_simplify_px: 0.0, // disable simplification for this test
			..Default::default()
		};
		let import = FeatureImport::from_features(vec![feature], config)?;

		let tile = import.get_tile(2, 1, 1)?.expect("tile in the polygon");
		assert_eq!(tile.layers.len(), 1);
		assert_eq!(tile.layers[0].features.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn from_source_loads_geojson() -> Result<()> {
		use crate::feature_source::GeoJsonSource;
		let src = GeoJsonSource::new("../testdata/places.geojson");
		let config = FeatureImportConfig {
			layer_name: "places".to_string(),
			max_zoom: 5,
			..Default::default()
		};
		let import = FeatureImport::from_source(&src, config).await?;
		assert!(import.bounds_mercator().is_some());

		let tile = import.get_tile(0, 0, 0)?.expect("world tile non-empty");
		assert_eq!(tile.layers[0].name, "places");
		// 4 input features but the MultiPolygon flattens to 2, total 5.
		assert_eq!(tile.layers[0].features.len(), 5);
		Ok(())
	}
}
