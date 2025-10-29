use super::{CompositeGeometryTrait, Coordinates, GeometryTrait, MultiLineStringGeometry, SingleGeometryTrait};
use anyhow::{Result, ensure};
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

#[derive(Clone, PartialEq)]
pub struct LineStringGeometry(pub Vec<Coordinates>);

impl GeometryTrait for LineStringGeometry {
	fn area(&self) -> f64 {
		0.0
	}

	fn verify(&self) -> Result<()> {
		ensure!(self.0.len() >= 2, "LineString must have at least two points");
		Ok(())
	}

	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		JsonValue::from(self.0.iter().map(|c| c.to_json(precision)).collect::<Vec<_>>())
	}
}

impl CompositeGeometryTrait<Coordinates> for LineStringGeometry {
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

impl SingleGeometryTrait<MultiLineStringGeometry> for LineStringGeometry {
	fn into_multi(self) -> MultiLineStringGeometry {
		MultiLineStringGeometry(vec![self])
	}
}

impl Debug for LineStringGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(LineStringGeometry, Coordinates);
