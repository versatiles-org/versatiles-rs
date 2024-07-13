use super::*;
use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct LineStringGeometry(pub Coordinates1);

impl LineStringGeometry {
	pub fn new(c: Vec<[f64; 2]>) -> Self {
		Self(c)
	}
}

impl SingleGeometryTrait<MultiLineStringGeometry> for LineStringGeometry {
	fn area(&self) -> f64 {
		0.0
	}

	fn into_multi(self) -> MultiLineStringGeometry {
		MultiLineStringGeometry(vec![self.0])
	}
}

impl VectorGeometryTrait<PointGeometry> for LineStringGeometry {
	fn into_iter(self) -> impl Iterator<Item = PointGeometry> {
		self.0.into_iter().map(PointGeometry)
	}

	fn len(&self) -> usize {
		self.0.len()
	}
}

impl Debug for LineStringGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

impl<T: Convertible> From<Vec<[T; 2]>> for LineStringGeometry {
	fn from(value: Vec<[T; 2]>) -> Self {
		Self(T::convert_coordinates1(value))
	}
}
