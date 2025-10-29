use super::{CompositeGeometryTrait, GeometryTrait, PolygonGeometry};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

#[derive(Clone, PartialEq)]
pub struct MultiPolygonGeometry(pub Vec<PolygonGeometry>);

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
}

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

impl Debug for MultiPolygonGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(MultiPolygonGeometry, PolygonGeometry);
