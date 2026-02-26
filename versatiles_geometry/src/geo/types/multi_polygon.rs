use super::{CompositeGeometryTrait, GeometryTrait, PolygonGeometry};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

/// Represents a collection of polygons, each of which may have an outer ring and optional inner holes.
/// This struct is used for complex, multi-part areas in 2D space.
#[derive(Clone, PartialEq)]
pub struct MultiPolygonGeometry(pub Vec<PolygonGeometry>);

/// Implementation of `GeometryTrait` for `MultiPolygonGeometry`.
///
/// - `area()` returns the sum of all polygon areas.
/// - `verify()` checks that each polygon is valid.
/// - `to_coord_json()` converts the geometry into a JSON array of polygons,
///   optionally rounding coordinates to a given precision.
impl GeometryTrait for MultiPolygonGeometry {
	fn area(&self) -> f64 {
		self.0.iter().map(super::traits::GeometryTrait::area).sum()
	}

	fn verify(&self) -> Result<()> {
		for line in &self.0 {
			line.verify()?;
		}
		Ok(())
	}

	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		JsonValue::from(
			self
				.0
				.iter()
				.map(|poly| poly.to_coord_json(precision))
				.collect::<Vec<_>>(),
		)
	}

	fn contains_point(&self, x: f64, y: f64) -> bool {
		self.0.iter().any(|poly| poly.contains_point(x, y))
	}

	fn to_mercator(&self) -> MultiPolygonGeometry {
		MultiPolygonGeometry(self.0.iter().map(PolygonGeometry::to_mercator).collect())
	}

	fn compute_bounds(&self) -> Option<[f64; 4]> {
		let mut x_min = f64::MAX;
		let mut y_min = f64::MAX;
		let mut x_max = f64::MIN;
		let mut y_max = f64::MIN;
		let mut has_bounds = false;

		for poly in &self.0 {
			if let Some(bounds) = poly.compute_bounds() {
				has_bounds = true;
				x_min = x_min.min(bounds[0]);
				y_min = y_min.min(bounds[1]);
				x_max = x_max.max(bounds[2]);
				y_max = y_max.max(bounds[3]);
			}
		}

		if has_bounds {
			Some([x_min, y_min, x_max, y_max])
		} else {
			None
		}
	}
}

/// Implementation of `CompositeGeometryTrait` for `MultiPolygonGeometry`.
///
/// Provides methods for working with the internal list of `PolygonGeometry` objects.
///
/// - `new()` creates an empty `MultiPolygonGeometry`.
/// - `as_vec()` returns an immutable reference to the internal polygons.
/// - `as_mut_vec()` returns a mutable reference to the internal polygons.
/// - `into_inner()` consumes the geometry and returns the vector of polygons.
impl CompositeGeometryTrait<PolygonGeometry> for MultiPolygonGeometry {
	fn new() -> Self {
		Self(Vec::new())
	}
	fn as_vec(&self) -> &Vec<PolygonGeometry> {
		&self.0
	}
	fn as_mut_vec(&mut self) -> &mut Vec<PolygonGeometry> {
		&mut self.0
	}
	fn into_inner(self) -> Vec<PolygonGeometry> {
		self.0
	}
}

/// Implementation of `Debug` for `MultiPolygonGeometry`.
///
/// Prints the list of polygons in a developer-friendly format.
impl Debug for MultiPolygonGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(MultiPolygonGeometry, PolygonGeometry);

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::super::RingGeometry;
	use super::*;

	fn single_square() -> MultiPolygonGeometry {
		MultiPolygonGeometry::from(&[[[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]]])
	}

	fn two_squares() -> MultiPolygonGeometry {
		MultiPolygonGeometry::from(&[
			[[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]],
			[[[20, 0], [30, 0], [30, 10], [20, 10], [20, 0]]],
		])
	}

	// ── GeometryTrait ───────────────────────────────────────────────────

	#[test]
	fn area_single() {
		assert_eq!(single_square().area(), 200.0);
	}

	#[test]
	fn area_multiple() {
		// Two 10x10 squares
		assert_eq!(two_squares().area(), 400.0);
	}

	#[test]
	fn area_empty() {
		assert_eq!(MultiPolygonGeometry::new().area(), 0.0);
	}

	#[test]
	fn verify_valid() {
		assert!(two_squares().verify().is_ok());
	}

	#[test]
	fn verify_empty_ok() {
		assert!(MultiPolygonGeometry::new().verify().is_ok());
	}

	#[test]
	fn verify_invalid_inner() {
		// Polygon with an invalid ring (only 3 points)
		let mut mp = MultiPolygonGeometry::new();
		let mut bad_poly = PolygonGeometry::new();
		bad_poly.push(RingGeometry::from(&[[0, 0], [1, 0], [0, 0]]));
		mp.push(bad_poly);
		assert!(mp.verify().is_err());
	}

	#[test]
	fn to_coord_json() {
		let json = two_squares().to_coord_json(None);
		let arr = json.as_array().unwrap();
		assert_eq!(arr.len(), 2);
	}

	#[test]
	fn contains_point_single() {
		assert!(single_square().contains_point(5.0, 5.0));
		assert!(!single_square().contains_point(-1.0, 5.0));
	}

	#[test]
	fn contains_point_multiple() {
		let mp = two_squares();
		assert!(mp.contains_point(5.0, 5.0)); // first polygon
		assert!(mp.contains_point(25.0, 5.0)); // second polygon
		assert!(!mp.contains_point(15.0, 5.0)); // between
	}

	#[test]
	fn contains_point_empty() {
		assert!(!MultiPolygonGeometry::new().contains_point(0.0, 0.0));
	}

	#[test]
	fn to_mercator() {
		let m = single_square().to_mercator();
		assert_eq!(m.0.len(), 1);
		// Non-origin points should be in meters
		assert!(m.0[0].0[0].0[1].x().abs() > 100_000.0);
	}

	#[test]
	fn compute_bounds() {
		let bounds = two_squares().compute_bounds().unwrap();
		assert_eq!(bounds, [0.0, 0.0, 30.0, 10.0]);
	}

	#[test]
	fn compute_bounds_empty() {
		assert!(MultiPolygonGeometry::new().compute_bounds().is_none());
	}

	// ── CompositeGeometryTrait ──────────────────────────────────────────

	#[test]
	fn composite_new_is_empty() {
		let mp = MultiPolygonGeometry::new();
		assert!(mp.is_empty());
		assert_eq!(mp.len(), 0);
	}

	#[test]
	fn composite_push_and_len() {
		let mut mp = MultiPolygonGeometry::new();
		mp.push(PolygonGeometry::from(&[[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]));
		assert_eq!(mp.len(), 1);
		assert!(!mp.is_empty());
	}

	#[test]
	fn composite_first_last() {
		let mp = two_squares();
		assert_eq!(mp.first().unwrap().0[0].0[0].x(), 0.0);
		assert_eq!(mp.last().unwrap().0[0].0[0].x(), 20.0);
	}

	#[test]
	fn composite_pop() {
		let mut mp = two_squares();
		let popped = mp.pop().unwrap();
		assert_eq!(popped.0[0].0[0].x(), 20.0);
		assert_eq!(mp.len(), 1);
	}

	#[test]
	fn composite_into_inner() {
		let inner = two_squares().into_inner();
		assert_eq!(inner.len(), 2);
	}

	#[test]
	fn composite_into_iter() {
		let polys: Vec<_> = two_squares().into_iter().collect();
		assert_eq!(polys.len(), 2);
	}

	#[test]
	fn composite_into_first_and_rest() {
		let (first, rest) = two_squares().into_first_and_rest().unwrap();
		assert_eq!(first.0[0].0[0].x(), 0.0);
		assert_eq!(rest.len(), 1);
	}

	#[test]
	fn composite_into_first_and_rest_empty() {
		assert!(MultiPolygonGeometry::new().into_first_and_rest().is_none());
	}

	// ── Debug / Clone / Eq ──────────────────────────────────────────────

	#[test]
	fn debug_format() {
		let mp = single_square();
		assert!(format!("{mp:?}").contains("[0.0, 0.0]"));
	}

	#[test]
	fn clone_and_eq() {
		let a = two_squares();
		assert_eq!(a.clone(), a);
	}

	#[test]
	fn ne() {
		assert_ne!(single_square(), two_squares());
	}

	// ── From conversions ────────────────────────────────────────────────

	#[test]
	fn from_vec() {
		let mp = MultiPolygonGeometry::from(vec![vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)]]]);
		assert_eq!(mp.len(), 1);
	}

	#[test]
	fn from_slice() {
		let data = [[[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)]]];
		let mp = MultiPolygonGeometry::from(&data[..]);
		assert_eq!(mp.len(), 1);
	}
}
