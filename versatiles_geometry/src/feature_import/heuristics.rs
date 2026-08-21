//! Heuristics for picking pipeline parameters from input data.
//!
//! Currently just one: [`auto_max_zoom`], which inspects the median feature
//! size and picks the zoom level where it renders at ≈ 4 tile-pixels.

use geo::{Area, Euclidean, Length};
use geo_types::Geometry;
use versatiles_core::WORLD_SIZE;

use super::TILE_EXTENT;
use crate::{ext::MercatorExt, geo::GeoFeature};

/// Pick the `max_zoom` where the median feature size renders at ≈ 4 tile-pixels.
///
/// Input features must be in **WGS84 lon/lat**; they're projected internally.
///
/// A point counts as a feature of size zero, so a dataset that is at least half
/// points has a median of zero and lands on the hard cap `MAX_ZOOM = 14`. Point
/// density isn't taken into account in v1 (known limitation; future versions may
/// use median nearest-neighbor distance) — but points are not *excluded* from the
/// median either, or one polygon among fifty points would set the zoom for all of
/// them (issue #242).
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
		.collect();
	if sizes.is_empty() {
		return MAX_ZOOM;
	}
	sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	// Upper median for an even count: `[a, b]` yields `b`.
	let median = sizes[sizes.len() / 2];

	// A median of zero means at least half the features are points, which carry
	// no scale to derive a zoom from — the same situation as a point-only input,
	// and it gets the same answer.
	if median <= 0.0 {
		return MAX_ZOOM;
	}

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

/// The size a feature contributes to the median, in mercator meters.
///
/// `None` means "leave this feature out of the median" — not "size zero".
fn feature_size_mercator(g: &Geometry<f64>) -> Option<f64> {
	let size = match g {
		// A point has no extent, but it is still a feature whose size is known:
		// zero. Leaving points out let a single polygon added to a point dataset
		// decide the median on its own, collapsing max_zoom to 0 (issue #242).
		Geometry::Point(_) | Geometry::MultiPoint(_) => return Some(0.0),
		Geometry::LineString(ls) => Euclidean.length(ls),
		Geometry::MultiLineString(ml) => Euclidean.length(ml),
		Geometry::Polygon(p) => p.unsigned_area().sqrt(),
		Geometry::MultiPolygon(mp) => mp.unsigned_area().sqrt(),
		_ => return None,
	};
	// A zero-length line or zero-area polygon is a data defect, not a zero-size
	// feature — unlike a point, it has no size *because it is broken*. Keep those
	// out of the median rather than dragging it to zero.
	(size.is_finite() && size > 0.0).then_some(size)
}

#[cfg(test)]
mod tests {
	use geo_types::{LineString, Point, Polygon};

	use super::*;

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
		// Points are size zero, so the median is zero — no scale to derive a
		// zoom from, and we fall back to the cap. Documented limitation.
		let features = vec![point_feature(0.0, 0.0), point_feature(13.4, 52.5)];
		assert_eq!(auto_max_zoom(&features), 14);
	}

	#[test]
	fn one_polygon_does_not_outvote_a_point_dataset() {
		// Issue #242: points used to be dropped before the median, so this
		// single world-spanning polygon was the whole sample and the answer
		// collapsed from 14 to 0.
		let mut features: Vec<GeoFeature> = (0..50).map(|i| point_feature(f64::from(i), 0.0)).collect();
		assert_eq!(auto_max_zoom_projected(&features), 14);

		features.push(square_polygon_feature(WORLD_SIZE));
		assert_eq!(auto_max_zoom_projected(&features), 14);
	}

	#[test]
	fn a_few_points_do_not_outvote_a_polygon_dataset() {
		// The mirror case: points are counted, not decisive. With 50 large
		// polygons and one point the median is still a polygon.
		let mut features: Vec<GeoFeature> = (0..50).map(|_| square_polygon_feature(WORLD_SIZE)).collect();
		features.push(point_feature(0.0, 0.0));
		assert_eq!(auto_max_zoom_projected(&features), 0);
	}

	#[test]
	fn an_even_split_takes_the_upper_median() {
		// `sizes[len / 2]` is the upper median, so an even mix of points and
		// polygons is decided by the polygons, not by the zero-size points.
		let features = vec![point_feature(0.0, 0.0), square_polygon_feature(WORLD_SIZE)];
		assert_eq!(auto_max_zoom_projected(&features), 0);
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
		// A degenerate (zero-length) linestring is still filtered out entirely —
		// it has no size *because it is broken*, unlike a point, which is
		// genuinely size zero and does count. Only the valid entry drives the
		// result.
		let zero_len = ls_feature(vec![[0.0, 0.0], [0.0, 0.0]]); // length = 0
		let one_m = ls_feature(vec![[0.0, 0.0], [1.0, 0.0]]); // length = 1m
		let result = auto_max_zoom_projected(&[zero_len, one_m]);
		// 1m line → very small relative to 4 px; pinned at MAX_ZOOM.
		assert_eq!(result, 14);
	}
}
