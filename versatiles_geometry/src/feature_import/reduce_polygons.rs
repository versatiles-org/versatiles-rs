//! Per-zoom polygon reduction: drop polygons whose unsigned area is below the
//! configured threshold (in mercator-meters², converted from tile-pixels² by
//! the caller).

use geo::Area;
use geo_types::Geometry;

/// Returns `true` if the geometry is *not* a polygon (kept unconditionally) or
/// is a polygon whose area is at least `min_area`. Returns `false` when an
/// areal geometry is below the threshold.
///
/// `min_area` is interpreted in the same coordinate units as the geometry —
/// callers convert from tile-pixels² to mercator-meters² before invoking.
#[must_use]
pub fn passes_min_area(g: &Geometry<f64>, min_area: f64) -> bool {
	if min_area <= 0.0 {
		return true;
	}
	match g {
		Geometry::Polygon(p) => p.unsigned_area() >= min_area,
		Geometry::MultiPolygon(mp) => mp.unsigned_area() >= min_area,
		// Non-areal geometries pass; the caller filters them via other thresholds.
		_ => true,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{LineString, Point, Polygon};

	fn square(side: f64) -> Polygon<f64> {
		Polygon::new(
			LineString::from(vec![[0.0, 0.0], [side, 0.0], [side, side], [0.0, side], [0.0, 0.0]]),
			vec![],
		)
	}

	#[test]
	fn drops_small_polygon() {
		let g: Geometry<f64> = Geometry::Polygon(square(1.0)); // area = 1
		assert!(!passes_min_area(&g, 4.0));
	}

	#[test]
	fn keeps_large_polygon() {
		let g: Geometry<f64> = Geometry::Polygon(square(10.0)); // area = 100
		assert!(passes_min_area(&g, 4.0));
	}

	#[test]
	fn zero_threshold_keeps_everything() {
		let g: Geometry<f64> = Geometry::Polygon(square(0.1));
		assert!(passes_min_area(&g, 0.0));
	}

	#[test]
	fn non_polygon_unaffected() {
		let g: Geometry<f64> = Geometry::Point(Point::new(0.0, 0.0));
		assert!(passes_min_area(&g, 100.0));
	}
}
