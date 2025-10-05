use super::*;
use anyhow::Result;
use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct MultiPointGeometry(pub Vec<PointGeometry>);

impl GeometryTrait for MultiPointGeometry {
	fn area(&self) -> f64 {
		0.0
	}

	fn verify(&self) -> Result<()> {
		for point in &self.0 {
			point.verify()?;
		}
		Ok(())
	}
}

impl CompositeGeometryTrait<PointGeometry> for MultiPointGeometry {
	fn new() -> Self {
		Self(Vec::new())
	}
	fn as_vec(&self) -> &Vec<PointGeometry> {
		&self.0
	}
	fn as_mut_vec(&mut self) -> &mut Vec<PointGeometry> {
		&mut self.0
	}
	fn into_inner(self) -> Vec<PointGeometry> {
		self.0
	}
}

impl Debug for MultiPointGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(MultiPointGeometry, PointGeometry);
