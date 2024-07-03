use super::*;
use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct MultiLineStringGeometry(pub Coordinates2);

impl MultiLineStringGeometry {
	pub fn new(c: Vec<Vec<[f64; 2]>>) -> Self {
		Self(c)
	}
}

impl MultiGeometryTrait for MultiLineStringGeometry {
	fn area(&self) -> f64 {
		0.0
	}
}

impl VectorGeometryTrait<LineStringGeometry> for MultiLineStringGeometry {
	fn into_iter(self) -> impl Iterator<Item = LineStringGeometry> {
		self.0.into_iter().map(LineStringGeometry)
	}

	fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	fn len(&self) -> usize {
		self.0.len()
	}
}

impl Debug for MultiLineStringGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

impl<T: Convertible> From<Vec<Vec<[T; 2]>>> for MultiLineStringGeometry {
	fn from(value: Vec<Vec<[T; 2]>>) -> Self {
		Self(T::convert_coordinates2(value))
	}
}
