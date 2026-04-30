//! Heuristics for picking pipeline parameters from input data.
//!
//! Currently just one: [`auto_max_zoom`], which inspects the median feature
//! size and picks the zoom level where it renders at ≈ 4 tile-pixels.

use super::TILE_EXTENT;
use crate::ext::MercatorExt;
use crate::geo::GeoFeature;
use geo::{Area, Euclidean, Length};
use geo_types::Geometry;
use versatiles_core::WORLD_SIZE;

/// Pick the `max_zoom` where the median feature size renders at ≈ 4 tile-pixels.
///
/// Input features must be in **WGS84 lon/lat**; they're projected internally.
/// For point-only inputs (no length/area to measure) the result is the
/// hard cap `MAX_ZOOM = 14` — point density isn't taken into account in v1
/// (known limitation; future versions may use median nearest-neighbor distance).
///
/// Most callers should set [`crate::feature_import::FeatureImportConfig::max_zoom`]
/// to `None` instead of calling this directly — that path projects each
/// geometry once instead of twice (this function clones every input geometry
/// to project it).
#[must_use]
pub fn auto_max_zoom(features_wgs84: &[GeoFeature]) -> u8 {
	let projected: Vec<GeoFeature> = features_wgs84
		.iter()
		.map(|f| GeoFeature {
			id: f.id.clone(),
			geometry: f.geometry.clone().to_mercator(),
			properties: f.properties.clone(),
		})
		.collect();
	auto_max_zoom_projected(&projected)
}

/// Internal: same as [`auto_max_zoom`] but expects features already in mercator.
/// Avoids an extra projection pass when the caller has them projected.
pub(super) fn auto_max_zoom_projected(features_mercator: &[GeoFeature]) -> u8 {
	const MAX_ZOOM: u8 = 14;
	const TARGET_PX: f64 = 4.0;

	let mut sizes: Vec<f64> = features_mercator
		.iter()
		.filter_map(|f| feature_size_mercator(&f.geometry))
		.filter(|s| s.is_finite() && *s > 0.0)
		.collect();
	if sizes.is_empty() {
		return MAX_ZOOM;
	}
	sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	let median = sizes[sizes.len() / 2];

	// At zoom z, a size of `median` mercator-meters renders as
	// `median * 2^z * TILE_EXTENT / WORLD_SIZE` pixels. Solve for the z where
	// that equals TARGET_PX:
	//     2^z = TARGET_PX * WORLD_SIZE / (median * TILE_EXTENT)
	//     z = log2(...)
	let zoom_f = (TARGET_PX * WORLD_SIZE / (median * f64::from(TILE_EXTENT))).log2();
	if !zoom_f.is_finite() {
		return MAX_ZOOM;
	}
	#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
	let z = zoom_f.round().clamp(0.0, f64::from(MAX_ZOOM)) as u8;
	z
}

fn feature_size_mercator(g: &Geometry<f64>) -> Option<f64> {
	match g {
		Geometry::LineString(ls) => Some(Euclidean.length(ls)),
		Geometry::MultiLineString(ml) => Some(Euclidean.length(ml)),
		Geometry::Polygon(p) => Some(p.unsigned_area().sqrt()),
		Geometry::MultiPolygon(mp) => Some(mp.unsigned_area().sqrt()),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{LineString, Point, Polygon};

	fn ls_feature(coords: Vec<[f64; 2]>) -> GeoFeature {
		GeoFeature::new(Geometry::LineString(LineString::from(coords)))
	}

	fn point_feature(x: f64, y: f64) -> GeoFeature {
		GeoFeature::new(Geometry::Point(Point::new(x, y)))
	}

	fn square_polygon_feature(side_meters: f64) -> GeoFeature {
		// Build directly in mercator coords so we can drive
		// `auto_max_zoom_projected` without depending on the projection.
		let s = side_meters;
		let exterior = LineString::from(vec![[0.0, 0.0], [s, 0.0], [s, s], [0.0, s], [0.0, 0.0]]);
		GeoFeature::new(Geometry::Polygon(Polygon::new(exterior, vec![])))
	}

	#[test]
	fn empty_input_returns_max_zoom() {
		// No features → no measurable sizes → fallback to the cap.
		assert_eq!(auto_max_zoom(&[]), 14);
	}

	#[test]
	fn point_only_input_returns_max_zoom() {
		// `feature_size_mercator` returns None for Point/MultiPoint, so the
		// median list is empty and we fall back to the cap. Documented
		// limitation.
		let features = vec![point_feature(0.0, 0.0), point_feature(13.4, 52.5)];
		assert_eq!(auto_max_zoom(&features), 14);
	}

	#[test]
	fn very_large_features_pick_zoom_zero() {
		// A polygon spanning the entire world projects to a size on the order
		// of WORLD_SIZE; the heuristic should clamp to 0.
		let big = square_polygon_feature(WORLD_SIZE);
		assert_eq!(auto_max_zoom_projected(&[big]), 0);
	}

	#[test]
	fn tiny_features_pick_max_zoom() {
		// A 1-meter-square polygon is much smaller than ~4 px even at zoom 14;
		// the heuristic clamps to MAX_ZOOM.
		let tiny = square_polygon_feature(1.0);
		assert_eq!(auto_max_zoom_projected(&[tiny]), 14);
	}

	#[test]
	fn skips_non_finite_and_zero_sizes() {
		// A degenerate (zero-length) linestring should be filtered out as
		// "no measurable size", and the only valid entry drives the result.
		let zero_len = ls_feature(vec![[0.0, 0.0], [0.0, 0.0]]); // length = 0
		let one_m = ls_feature(vec![[0.0, 0.0], [1.0, 0.0]]); // length = 1m
		let result = auto_max_zoom_projected(&[zero_len, one_m]);
		// 1m line → very small relative to 4 px; pinned at MAX_ZOOM.
		assert_eq!(result, 14);
	}
}
