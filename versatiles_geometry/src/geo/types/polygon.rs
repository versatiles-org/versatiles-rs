use super::*;
use crate::math;
use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct PolygonGeometry(pub Coordinates2);

impl PolygonGeometry {
	pub fn new(c: Vec<Vec<[f64; 2]>>) -> Self {
		Self(c)
	}
}

impl SingleGeometryTrait<MultiPolygonGeometry> for PolygonGeometry {
	fn area(&self) -> f64 {
		math::area_polygon(&self.0)
	}

	fn into_multi(self) -> MultiPolygonGeometry {
		MultiPolygonGeometry(vec![self.0])
	}
}

impl VectorGeometryTrait<RingGeometry> for PolygonGeometry {
	fn into_iter(self) -> impl Iterator<Item = RingGeometry> {
		self.0.into_iter().map(RingGeometry)
	}

	fn len(&self) -> usize {
		self.0.len()
	}
}

impl Debug for PolygonGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

impl<T: Convertible> From<Vec<Vec<[T; 2]>>> for PolygonGeometry {
	fn from(value: Vec<Vec<[T; 2]>>) -> Self {
		Self(T::convert_coordinates2(value))
	}
}
impl<T: Convertible> From<Vec<[T; 2]>> for PolygonGeometry {
	fn from(value: Vec<[T; 2]>) -> Self {
		Self(vec![T::convert_coordinates1(value)])
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_area() {
		let polygon = PolygonGeometry::from(vec![
			[0.0, 0.0],
			[5.0, 0.0],
			[5.0, 5.0],
			[0.0, 5.0],
			[0.0, 0.0],
		]);
		let area = polygon.area();
		assert_eq!(area, 50.0);
	}
}
