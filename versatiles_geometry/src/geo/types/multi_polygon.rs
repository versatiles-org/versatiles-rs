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

	fn compute_bounds(&self) -> [f64; 4] {
		let mut x_min = f64::MAX;
		let mut y_min = f64::MAX;
		let mut x_max = f64::MIN;
		let mut y_max = f64::MIN;

		for poly in &self.0 {
			let bounds = poly.compute_bounds();
			x_min = x_min.min(bounds[0]);
			y_min = y_min.min(bounds[1]);
			x_max = x_max.max(bounds[2]);
			y_max = y_max.max(bounds[3]);
		}

		[x_min, y_min, x_max, y_max]
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
mod tests {
	use super::*;

	#[test]
	fn test_contains_point_single_polygon() {
		let multi = MultiPolygonGeometry::from(&[[[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]]]);

		assert!(multi.contains_point(5.0, 5.0));
		assert!(!multi.contains_point(-1.0, 5.0));
	}

	#[test]
	fn test_contains_point_multiple_polygons() {
		// Two separate squares
		let multi = MultiPolygonGeometry::from(&[
			[[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]],
			[[[20, 0], [30, 0], [30, 10], [20, 10], [20, 0]]],
		]);

		// Inside first polygon
		assert!(multi.contains_point(5.0, 5.0));

		// Inside second polygon
		assert!(multi.contains_point(25.0, 5.0));

		// Between polygons (outside both)
		assert!(!multi.contains_point(15.0, 5.0));
	}

	#[test]
	fn test_contains_point_empty() {
		let multi = MultiPolygonGeometry::new();
		assert!(!multi.contains_point(0.0, 0.0));
	}

	#[test]
	fn test_to_mercator() {
		let multi = MultiPolygonGeometry::from(&[[[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]]);
		let mercator = multi.to_mercator();

		// Check that we have the same number of polygons
		assert_eq!(mercator.0.len(), 1);

		// Origin should still be at (0, 0) in mercator
		assert!(mercator.0[0].0[0].0[0].x().abs() < 1.0);
		assert!(mercator.0[0].0[0].0[0].y().abs() < 1.0);

		// Non-origin points should be in meters (much larger values)
		assert!(mercator.0[0].0[0].0[1].x().abs() > 100_000.0);
	}

	#[test]
	fn test_compute_bounds() {
		let multi = MultiPolygonGeometry::from(&[
			[[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]],
			[[[20, 5], [30, 5], [30, 15], [20, 15], [20, 5]]],
		]);
		let bounds = multi.compute_bounds();

		assert!((bounds[0] - 0.0).abs() < 1e-10); // x_min
		assert!((bounds[1] - 0.0).abs() < 1e-10); // y_min
		assert!((bounds[2] - 30.0).abs() < 1e-10); // x_max
		assert!((bounds[3] - 15.0).abs() < 1e-10); // y_max
	}

	#[test]
	fn test_compute_bounds_empty() {
		let multi = MultiPolygonGeometry::new();
		let bounds = multi.compute_bounds();

		assert_eq!(bounds[0], f64::MAX);
		assert_eq!(bounds[1], f64::MAX);
		assert_eq!(bounds[2], f64::MIN);
		assert_eq!(bounds[3], f64::MIN);
	}
}
