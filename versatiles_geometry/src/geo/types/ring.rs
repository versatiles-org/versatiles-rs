use super::*;
use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct RingGeometry(pub Coordinates1);

impl RingGeometry {
	pub fn new(c: Vec<[f64; 2]>) -> Self {
		Self(c)
	}
}

impl VectorGeometryTrait<PointGeometry> for RingGeometry {
	fn into_iter(self) -> impl Iterator<Item = PointGeometry> {
		self.0.into_iter().map(PointGeometry)
	}

	fn len(&self) -> usize {
		self.0.len()
	}
}

impl Debug for RingGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

impl<T: Convertible> From<Vec<[T; 2]>> for RingGeometry {
	fn from(value: Vec<[T; 2]>) -> Self {
		Self(T::convert_coordinates1(value))
	}
}
