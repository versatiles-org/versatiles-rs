use super::{CompositeGeometryTrait, GeometryTrait, MultiPolygonGeometry, RingGeometry, SingleGeometryTrait};
use anyhow::{Result, ensure};
use std::{fmt::Debug, vec};
use versatiles_core::json::JsonValue;

/// Represents a polygon composed of one or more closed rings.
///
/// The first ring in the vector is considered the outer boundary of the polygon,
/// while any subsequent rings represent holes within the polygon.
/// This structure is commonly used for representing areas and regions in 2D space.
#[derive(Clone, PartialEq)]
pub struct PolygonGeometry(pub Vec<RingGeometry>);

impl GeometryTrait for PolygonGeometry {
	/// Calculates the area of the polygon.
	///
	/// The area is computed by summing the area of the outer ring and subtracting
	/// the areas of any inner rings (holes).
	fn area(&self) -> f64 {
		let mut outer = true;
		let mut sum = 0.0;
		for ring in &self.0 {
			if outer {
				sum = ring.area();
				outer = false;
			} else {
				sum -= ring.area();
			}
		}
		sum
	}

	/// Verifies the validity of the polygon.
	///
	/// Ensures that the polygon has at least one ring and that all rings are valid.
	fn verify(&self) -> Result<()> {
		ensure!(!self.0.is_empty(), "Polygon must have at least one ring");
		for ring in &self.0 {
			ring.verify()?;
		}
		Ok(())
	}

	/// Converts the polygon into a JSON array of coordinate rings.
	///
	/// Each ring is converted into its coordinate representation, optionally rounded
	/// to the specified precision.
	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		JsonValue::from(
			self
				.0
				.iter()
				.map(|ring| ring.to_coord_json(precision))
				.collect::<Vec<_>>(),
		)
	}

	fn contains_point(&self, x: f64, y: f64) -> bool {
		if self.0.is_empty() {
			return false;
		}

		// Must be inside the exterior ring (first ring)
		if !self.0[0].contains_point(x, y) {
			return false;
		}

		// Must not be inside any holes (subsequent rings)
		for hole in self.0.iter().skip(1) {
			if hole.contains_point(x, y) {
				return false;
			}
		}

		true
	}

	fn to_mercator(&self) -> PolygonGeometry {
		PolygonGeometry(self.0.iter().map(RingGeometry::to_mercator).collect())
	}

	fn compute_bounds(&self) -> Option<[f64; 4]> {
		self.0.first().and_then(GeometryTrait::compute_bounds)
	}
}

impl SingleGeometryTrait<MultiPolygonGeometry> for PolygonGeometry {
	/// Wraps this polygon into a `MultiPolygonGeometry`.
	fn into_multi(self) -> MultiPolygonGeometry {
		MultiPolygonGeometry(vec![self])
	}
}

impl CompositeGeometryTrait<RingGeometry> for PolygonGeometry {
	/// Creates a new, empty `PolygonGeometry`.
	fn new() -> Self {
		Self(Vec::new())
	}
	/// Returns a reference to the vector of `RingGeometry` elements.
	fn as_vec(&self) -> &Vec<RingGeometry> {
		&self.0
	}
	/// Returns a mutable reference to the vector of `RingGeometry` elements.
	fn as_mut_vec(&mut self) -> &mut Vec<RingGeometry> {
		&mut self.0
	}
	/// Consumes the polygon and returns the internal vector of rings.
	fn into_inner(self) -> Vec<RingGeometry> {
		self.0
	}
}

impl Debug for PolygonGeometry {
	/// Formats the polygon for debugging by printing its list of rings.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(PolygonGeometry, RingGeometry);

impl From<geo::Polygon<f64>> for PolygonGeometry {
	/// Converts a `geo::Polygon` into a `PolygonGeometry`, preserving the outer and inner rings.
	fn from(geometry: geo::Polygon<f64>) -> Self {
		let (exterior, interiors) = geometry.into_inner();
		let mut rings = Vec::with_capacity(interiors.len() + 1);
		rings.push(RingGeometry::from(exterior));
		for interior in interiors {
			rings.push(RingGeometry::from(interior));
		}
		PolygonGeometry(rings)
	}
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;

	#[test]
	fn test_area() {
		let polygon = PolygonGeometry::from(&[[[0, 0], [5, 0], [5, 5], [0, 5], [0, 0]]]);
		let area = polygon.area();
		assert_eq!(area, 50.0);
	}

	#[test]
	fn test_contains_point_simple() {
		let polygon = PolygonGeometry::from(&[[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]]);

		// Inside
		assert!(polygon.contains_point(5.0, 5.0));

		// Outside
		assert!(!polygon.contains_point(-1.0, 5.0));
		assert!(!polygon.contains_point(15.0, 5.0));
	}

	#[test]
	fn test_contains_point_with_hole() {
		// Outer ring: (0,0) to (20,20)
		// Hole: (5,5) to (15,15)
		let polygon = PolygonGeometry::from(&[
			[[0, 0], [20, 0], [20, 20], [0, 20], [0, 0]],
			[[5, 5], [15, 5], [15, 15], [5, 15], [5, 5]],
		]);

		// Outside outer ring
		assert!(!polygon.contains_point(-1.0, 10.0));

		// Inside outer ring, outside hole
		assert!(polygon.contains_point(2.0, 2.0));
		assert!(polygon.contains_point(18.0, 18.0));

		// Inside hole (should be false)
		assert!(!polygon.contains_point(10.0, 10.0));
	}

	#[test]
	fn test_contains_point_empty() {
		let polygon = PolygonGeometry::new();
		assert!(!polygon.contains_point(0.0, 0.0));
	}

	#[test]
	fn test_to_mercator() {
		// Use integers only (the from impl expects integers)
		let polygon = PolygonGeometry::from(&[[[-1, -1], [1, -1], [1, 1], [-1, 1], [-1, -1]]]);
		let mercator = polygon.to_mercator();

		// Check that we have the same number of rings
		assert_eq!(mercator.0.len(), 1);

		// Check that the coordinates are now in meters (much larger values)
		assert!(mercator.0[0].0[0].x().abs() > 100_000.0);
	}
}
