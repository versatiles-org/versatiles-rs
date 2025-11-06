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
mod tests {
	use super::*;

	#[test]
	fn test_area() {
		let polygon = PolygonGeometry::from(&[[[0, 0], [5, 0], [5, 5], [0, 5], [0, 0]]]);
		let area = polygon.area();
		assert_eq!(area, 50.0);
	}
}
