use super::{LineStringGeometry, GeometryTrait, CompositeGeometryTrait};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

#[derive(Clone, PartialEq)]
pub struct MultiLineStringGeometry(pub Vec<LineStringGeometry>);

impl GeometryTrait for MultiLineStringGeometry {
	fn area(&self) -> f64 {
		0.0
	}

	fn verify(&self) -> Result<()> {
		for line in &self.0 {
			line.verify()?;
		}
		Ok(())
	}

	fn to_coord_json(&self) -> JsonValue {
		JsonValue::from(self.0.iter().map(super::traits::GeometryTrait::to_coord_json).collect::<Vec<_>>())
	}
}

impl CompositeGeometryTrait<LineStringGeometry> for MultiLineStringGeometry {
	fn new() -> Self {
		Self(Vec::new())
	}
	fn as_vec(&self) -> &Vec<LineStringGeometry> {
		&self.0
	}
	fn as_mut_vec(&mut self) -> &mut Vec<LineStringGeometry> {
		&mut self.0
	}
	fn into_inner(self) -> Vec<LineStringGeometry> {
		self.0
	}
}

impl Debug for MultiLineStringGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(MultiLineStringGeometry, LineStringGeometry);
