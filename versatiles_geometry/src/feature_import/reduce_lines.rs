//! Per-zoom line reduction: drop linestrings whose total length is below the
//! configured threshold (in mercator-meters, converted from tile-pixels by
//! the caller).

use geo::{Euclidean, Length};
use geo_types::Geometry;

/// Returns `true` if the geometry is *not* a line (kept unconditionally) or is
/// a linestring whose total length is at least `min_length`. Returns `false`
/// when a line geometry is below the threshold.
///
/// `min_length` is interpreted in the same coordinate units as the geometry —
/// callers convert from tile-pixels to mercator-meters before invoking.
#[must_use]
pub fn passes_min_length(g: &Geometry<f64>, min_length: f64) -> bool {
	if min_length <= 0.0 {
		return true;
	}
	match g {
		Geometry::LineString(ls) => Euclidean.length(ls) >= min_length,
		Geometry::MultiLineString(ml) => Euclidean.length(ml) >= min_length,
		// Non-line geometries pass; the caller filters them via other thresholds.
		_ => true,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{LineString, Point};

	#[test]
	fn drops_short_line() {
		let ls = LineString::from(vec![[0.0, 0.0], [1.0, 0.0]]); // length = 1
		let g: Geometry<f64> = Geometry::LineString(ls);
		assert!(!passes_min_length(&g, 4.0));
	}

	#[test]
	fn keeps_long_line() {
		let ls = LineString::from(vec![[0.0, 0.0], [10.0, 0.0]]); // length = 10
		let g: Geometry<f64> = Geometry::LineString(ls);
		assert!(passes_min_length(&g, 4.0));
	}

	#[test]
	fn zero_threshold_keeps_everything() {
		let ls = LineString::from(vec![[0.0, 0.0], [0.5, 0.0]]);
		assert!(passes_min_length(&Geometry::LineString(ls), 0.0));
	}

	#[test]
	fn non_line_unaffected() {
		let g: Geometry<f64> = Geometry::Point(Point::new(0.0, 0.0));
		assert!(passes_min_length(&g, 100.0));
	}
}
