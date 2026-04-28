//! Structural validation for `geo_types` geometries.
//!
//! `geo_types` deliberately allows malformed geometries (empty linestrings, unclosed
//! polygon rings, etc.). Versatiles validates at its boundaries (parsers, importers,
//! encoders) using this free function.

use anyhow::{Result, bail};
use geo_types::{Geometry, GeometryCollection, LineString, MultiLineString, MultiPolygon, Polygon};

/// Validate that a geometry has the structural shape versatiles expects.
///
/// Rules:
/// - `LineString` must have at least two points.
/// - `Polygon` must have at least one ring; every ring is closed and has ≥4 points.
/// - `MultiLineString`/`MultiPolygon` validate each child.
/// - `Point`/`MultiPoint`/`Line`/`Rect`/`Triangle` are always considered valid (they
///   cannot be malformed under their type definitions, modulo NaN coordinates which
///   we do not check here).
/// - `GeometryCollection` validates each child recursively.
pub fn validate(g: &Geometry<f64>) -> Result<()> {
	match g {
		Geometry::LineString(ls) => validate_line_string(ls),
		Geometry::Polygon(p) => validate_polygon(p),
		Geometry::MultiLineString(ml) => validate_multi_line_string(ml),
		Geometry::MultiPolygon(mp) => validate_multi_polygon(mp),
		Geometry::GeometryCollection(gc) => validate_geometry_collection(gc),
		Geometry::Point(_) | Geometry::Line(_) | Geometry::MultiPoint(_) | Geometry::Rect(_) | Geometry::Triangle(_) => {
			Ok(())
		}
	}
}

fn validate_line_string(ls: &LineString<f64>) -> Result<()> {
	if ls.0.len() < 2 {
		bail!("LineString must have at least 2 points, found {}", ls.0.len());
	}
	Ok(())
}

fn validate_ring(ring: &LineString<f64>) -> Result<()> {
	if ring.0.len() < 4 {
		bail!("polygon ring must have at least 4 points, found {}", ring.0.len());
	}
	let first = ring.0.first().expect("len >= 4");
	let last = ring.0.last().expect("len >= 4");
	if first != last {
		bail!("polygon ring must be closed (first point must equal last)");
	}
	Ok(())
}

fn validate_polygon(p: &Polygon<f64>) -> Result<()> {
	validate_ring(p.exterior())?;
	for interior in p.interiors() {
		validate_ring(interior)?;
	}
	Ok(())
}

fn validate_multi_line_string(ml: &MultiLineString<f64>) -> Result<()> {
	for ls in &ml.0 {
		validate_line_string(ls)?;
	}
	Ok(())
}

fn validate_multi_polygon(mp: &MultiPolygon<f64>) -> Result<()> {
	for p in &mp.0 {
		validate_polygon(p)?;
	}
	Ok(())
}

fn validate_geometry_collection(gc: &GeometryCollection<f64>) -> Result<()> {
	for g in &gc.0 {
		validate(g)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{Coord, LineString, Point, Polygon};

	#[test]
	fn point_is_always_valid() {
		assert!(validate(&Geometry::Point(Point::new(0.0, 0.0))).is_ok());
	}

	#[test]
	fn line_string_too_short_fails() {
		let ls = LineString::from(vec![Coord { x: 0.0, y: 0.0 }]);
		let g: Geometry<f64> = Geometry::LineString(ls);
		assert!(validate(&g).is_err());
	}

	#[test]
	fn line_string_two_points_ok() {
		let ls = LineString::from(vec![[0.0, 0.0], [1.0, 1.0]]);
		assert!(validate(&Geometry::LineString(ls)).is_ok());
	}

	#[test]
	fn polygon_too_few_points_fails() {
		// `Polygon::new` auto-closes its rings, so the only way to produce a malformed
		// ring is to give too few input points (≤ 2 unique → ≤ 3 after auto-close).
		let exterior = LineString::from(vec![[0.0, 0.0], [1.0, 1.0]]);
		let p = Polygon::new(exterior, vec![]);
		assert!(validate(&Geometry::Polygon(p)).is_err());
	}

	#[test]
	fn closed_polygon_ring_ok() {
		let exterior = LineString::from(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]);
		let p = Polygon::new(exterior, vec![]);
		assert!(validate(&Geometry::Polygon(p)).is_ok());
	}

	#[test]
	fn polygon_with_invalid_hole_fails() {
		let exterior = LineString::from(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 0.0]]);
		let interior = LineString::from(vec![[1.0, 1.0], [2.0, 2.0]]); // only 2 points, not closed
		let p = Polygon::new(exterior, vec![interior]);
		assert!(validate(&Geometry::Polygon(p)).is_err());
	}
}
