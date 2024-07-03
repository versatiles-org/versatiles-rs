use super::*;
use crate::math;
use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct MultiPolygonGeometry(pub Coordinates3);

impl MultiPolygonGeometry {
	pub fn new(c: Vec<Vec<Vec<[f64; 2]>>>) -> Self {
		Self(c)
	}
}

impl MultiGeometryTrait for MultiPolygonGeometry {
	fn area(&self) -> f64 {
		math::area_multi_polygon(&self.0)
	}
}

impl VectorGeometryTrait<PolygonGeometry> for MultiPolygonGeometry {
	fn into_iter(self) -> impl Iterator<Item = PolygonGeometry> {
		self.0.into_iter().map(PolygonGeometry)
	}

	fn len(&self) -> usize {
		self.0.len()
	}
}

impl Debug for MultiPolygonGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

impl<T: Convertible> From<Vec<Vec<Vec<[T; 2]>>>> for MultiPolygonGeometry {
	fn from(value: Vec<Vec<Vec<[T; 2]>>>) -> Self {
		Self(T::convert_coordinates3(value))
	}
}
