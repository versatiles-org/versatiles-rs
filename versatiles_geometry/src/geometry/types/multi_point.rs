use traits::MultiGeometryTrait;

use super::*;
use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct MultiPointGeometry(pub Coordinates1);

impl MultiPointGeometry {
	pub fn new(c: Vec<[f64; 2]>) -> Self {
		Self(c)
	}
}

impl MultiGeometryTrait for MultiPointGeometry {
	fn area(&self) -> f64 {
		0.0
	}
}

impl VectorGeometryTrait<PointGeometry> for MultiPointGeometry {
	fn into_iter(self) -> impl Iterator<Item = PointGeometry> {
		self.0.into_iter().map(PointGeometry)
	}

	fn len(&self) -> usize {
		self.0.len()
	}
}

impl Debug for MultiPointGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

impl<T: Convertible> From<Vec<[T; 2]>> for MultiPointGeometry {
	fn from(value: Vec<[T; 2]>) -> Self {
		Self(T::convert_coordinates1(value))
	}
}
