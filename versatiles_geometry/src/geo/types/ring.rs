use super::{Coordinates, GeometryTrait, CompositeGeometryTrait};
use anyhow::{Result, ensure};
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

#[derive(Clone, PartialEq)]
pub struct RingGeometry(pub Vec<Coordinates>);

impl GeometryTrait for RingGeometry {
	fn area(&self) -> f64 {
		let mut sum = 0f64;
		let mut p2 = self.0.last().unwrap();
		for p1 in &self.0 {
			sum += (p2.x() - p1.x()) * (p1.y() + p2.y());
			p2 = p1;
		}
		sum
	}

	fn verify(&self) -> Result<()> {
		ensure!(self.0.len() >= 4, "Ring must have at least 4 points");
		ensure!(self.0.first() == self.0.last(), "Ring must be closed");
		Ok(())
	}

	fn to_coord_json(&self) -> JsonValue {
		JsonValue::from(self.0.iter().map(super::coordinates::Coordinates::to_json).collect::<Vec<_>>())
	}
}

impl CompositeGeometryTrait<Coordinates> for RingGeometry {
	fn new() -> Self {
		Self(Vec::new())
	}
	fn as_vec(&self) -> &Vec<Coordinates> {
		&self.0
	}
	fn as_mut_vec(&mut self) -> &mut Vec<Coordinates> {
		&mut self.0
	}
	fn into_inner(self) -> Vec<Coordinates> {
		self.0
	}
}

impl Debug for RingGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(RingGeometry, Coordinates);

impl From<geo::LineString<f64>> for RingGeometry {
	fn from(geometry: geo::LineString<f64>) -> Self {
		RingGeometry(geometry.into_iter().map(Coordinates::from).collect())
	}
}
