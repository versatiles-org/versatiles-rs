use super::*;
use anyhow::{Result, ensure};
use std::{fmt::Debug, vec};
use versatiles_core::json::JsonValue;

#[derive(Clone, PartialEq)]
pub struct PolygonGeometry(pub Vec<RingGeometry>);

impl GeometryTrait for PolygonGeometry {
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

	fn verify(&self) -> Result<()> {
		ensure!(!self.0.is_empty(), "Polygon must have at least one ring");
		for ring in &self.0 {
			ring.verify()?;
		}
		Ok(())
	}

	fn to_coord_json(&self) -> JsonValue {
		JsonValue::from(self.0.iter().map(|c| c.to_coord_json()).collect::<Vec<_>>())
	}
}

impl SingleGeometryTrait<MultiPolygonGeometry> for PolygonGeometry {
	fn into_multi(self) -> MultiPolygonGeometry {
		MultiPolygonGeometry(vec![self])
	}
}

impl CompositeGeometryTrait<RingGeometry> for PolygonGeometry {
	fn new() -> Self {
		Self(Vec::new())
	}
	fn as_vec(&self) -> &Vec<RingGeometry> {
		&self.0
	}
	fn as_mut_vec(&mut self) -> &mut Vec<RingGeometry> {
		&mut self.0
	}
	fn into_inner(self) -> Vec<RingGeometry> {
		self.0
	}
}

impl Debug for PolygonGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(PolygonGeometry, RingGeometry);

impl From<geo::Polygon<f64>> for PolygonGeometry {
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
