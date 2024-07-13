use super::*;
use std::fmt::Debug;
use traits::SingleGeometryTrait;

#[derive(Clone, PartialEq)]
pub struct PointGeometry(pub Coordinates0);

impl PointGeometry {
	#[allow(dead_code)]
	fn new(c: [f64; 2]) -> Self {
		Self(c)
	}
}

impl SingleGeometryTrait<MultiPointGeometry> for PointGeometry {
	fn area(&self) -> f64 {
		0.0
	}

	fn into_multi(self) -> MultiPointGeometry {
		MultiPointGeometry(vec![self.0])
	}
}

impl Debug for PointGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(self.0).finish()
	}
}

impl<T: Convertible> From<[T; 2]> for PointGeometry {
	fn from(value: [T; 2]) -> Self {
		Self(T::convert_coordinates0(value))
	}
}

impl<T: Convertible> From<&[T; 2]> for PointGeometry {
	fn from(value: &[T; 2]) -> Self {
		Self(T::convert_coordinates0(*value))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_point_geometry_new() {
		let point = PointGeometry::new([1.0, 2.0]);
		assert_eq!(point.0[0], 1.0);
		assert_eq!(point.0[1], 2.0);
	}

	#[test]
	fn test_point_geometry_eq() {
		let point1 = PointGeometry::new([1.0, 2.0]);
		let point2 = PointGeometry::new([1.0, 2.0]);
		let point3 = PointGeometry::new([3.0, 4.0]);
		assert_eq!(point1, point2);
		assert_ne!(point1, point3);
	}

	#[test]
	fn test_point_geometry_debug() {
		let point = PointGeometry::new([1.0, 2.0]);
		assert_eq!(format!("{:?}", point), "[1.0, 2.0]");
	}

	#[test]
	fn test_point_geometry_from_f64_array_ref() {
		let arr = &[1.0, 2.0];
		let point: PointGeometry = PointGeometry::from(arr);
		assert_eq!(point.0[0], 1.0);
		assert_eq!(point.0[1], 2.0);
	}

	#[test]
	fn test_point_geometry_from_f64_array() {
		let arr = [1.0, 2.0];
		let point: PointGeometry = PointGeometry::from(arr);
		assert_eq!(point.0[0], 1.0);
		assert_eq!(point.0[1], 2.0);
	}

	#[test]
	fn test_point_geometry_from_i64_array() {
		let arr = [1, 2];
		let point: PointGeometry = PointGeometry::from(&arr);
		assert_eq!(point.0[0], 1.0);
		assert_eq!(point.0[1], 2.0);
	}
}
