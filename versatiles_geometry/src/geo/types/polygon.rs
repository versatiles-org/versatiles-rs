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

	fn square_poly() -> PolygonGeometry {
		PolygonGeometry::from(&[[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]])
	}

	// ── area ────────────────────────────────────────────────────────────

	#[test]
	fn area() {
		let polygon = PolygonGeometry::from(&[[[0, 0], [5, 0], [5, 5], [0, 5], [0, 0]]]);
		assert_eq!(polygon.area(), 50.0);
	}

	// ── verify ──────────────────────────────────────────────────────────

	#[test]
	fn verify_valid() {
		assert!(square_poly().verify().is_ok());
	}

	#[test]
	fn verify_empty() {
		assert!(PolygonGeometry::new().verify().is_err());
	}

	#[test]
	fn verify_invalid_ring() {
		// Ring with only 3 points (need >=4)
		let polygon = PolygonGeometry(vec![RingGeometry::from(&[[0, 0], [1, 0], [0, 0]])]);
		assert!(polygon.verify().is_err());
	}

	// ── to_coord_json ───────────────────────────────────────────────────

	#[test]
	fn to_coord_json() {
		let json = square_poly().to_coord_json(None);
		let rings = json.as_array().unwrap();
		assert_eq!(rings.len(), 1);
	}

	// ── contains_point ──────────────────────────────────────────────────

	#[test]
	fn contains_point_simple() {
		assert!(square_poly().contains_point(5.0, 5.0));
		assert!(!square_poly().contains_point(-1.0, 5.0));
		assert!(!square_poly().contains_point(15.0, 5.0));
	}

	#[test]
	fn contains_point_with_hole() {
		let polygon = PolygonGeometry::from(&[
			[[0, 0], [20, 0], [20, 20], [0, 20], [0, 0]],
			[[5, 5], [15, 5], [15, 15], [5, 15], [5, 5]],
		]);
		assert!(!polygon.contains_point(-1.0, 10.0)); // outside
		assert!(polygon.contains_point(2.0, 2.0)); // inside, outside hole
		assert!(!polygon.contains_point(10.0, 10.0)); // inside hole
	}

	#[test]
	fn contains_point_empty() {
		assert!(!PolygonGeometry::new().contains_point(0.0, 0.0));
	}

	// ── to_mercator ─────────────────────────────────────────────────────

	#[test]
	fn to_mercator() {
		let polygon = PolygonGeometry::from(&[[[-1, -1], [1, -1], [1, 1], [-1, 1], [-1, -1]]]);
		let m = polygon.to_mercator();
		assert_eq!(m.0.len(), 1);
		assert!(m.0[0].0[0].x().abs() > 100_000.0);
	}

	// ── compute_bounds ──────────────────────────────────────────────────

	#[test]
	fn compute_bounds() {
		let bounds = square_poly().compute_bounds().unwrap();
		assert_eq!(bounds, [0.0, 0.0, 10.0, 10.0]);
	}

	#[test]
	fn compute_bounds_empty() {
		assert!(PolygonGeometry::new().compute_bounds().is_none());
	}

	// ── into_multi ──────────────────────────────────────────────────────

	#[test]
	fn into_multi() {
		let p = square_poly();
		let multi = p.clone().into_multi();
		assert_eq!(multi.0.len(), 1);
		assert_eq!(multi.0[0], p);
	}

	// ── CompositeGeometryTrait ──────────────────────────────────────────

	#[test]
	fn composite_push_and_len() {
		let mut poly = PolygonGeometry::new();
		assert!(poly.is_empty());
		poly.push(RingGeometry::from(&[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]));
		assert_eq!(poly.len(), 1);
	}

	// ── Debug / Clone / Eq ──────────────────────────────────────────────

	#[test]
	fn debug_format() {
		let poly = square_poly();
		assert!(format!("{poly:?}").contains("[0.0, 0.0]"));
	}

	#[test]
	fn clone_and_eq() {
		let a = square_poly();
		assert_eq!(a.clone(), a);
	}
}
