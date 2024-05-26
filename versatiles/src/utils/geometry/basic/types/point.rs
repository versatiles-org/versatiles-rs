use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct PointGeometry {
	pub x: f64,
	pub y: f64,
}

impl PointGeometry {
	pub fn new(x: f64, y: f64) -> Self {
		Self { x, y }
	}
}

impl Eq for PointGeometry {}

impl Debug for PointGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entry(&self.x).entry(&self.y).finish()
	}
}

impl From<&[f64; 2]> for PointGeometry {
	fn from(value: &[f64; 2]) -> Self {
		Self {
			x: value[0],
			y: value[1],
		}
	}
}

impl From<[f64; 2]> for PointGeometry {
	fn from(value: [f64; 2]) -> Self {
		Self {
			x: value[0],
			y: value[1],
		}
	}
}

impl From<[i64; 2]> for PointGeometry {
	fn from(value: [i64; 2]) -> Self {
		Self {
			x: value[0] as f64,
			y: value[1] as f64,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_point_geometry_new() {
		let point = PointGeometry::new(1.0, 2.0);
		assert_eq!(point.x, 1.0);
		assert_eq!(point.y, 2.0);
	}

	#[test]
	fn test_point_geometry_eq() {
		let point1 = PointGeometry::new(1.0, 2.0);
		let point2 = PointGeometry::new(1.0, 2.0);
		let point3 = PointGeometry::new(3.0, 4.0);
		assert_eq!(point1, point2);
		assert_ne!(point1, point3);
	}

	#[test]
	fn test_point_geometry_debug() {
		let point = PointGeometry::new(1.0, 2.0);
		assert_eq!(format!("{:?}", point), "[1.0, 2.0]");
	}

	#[test]
	fn test_point_geometry_from_f64_array_ref() {
		let arr = &[1.0, 2.0];
		let point: PointGeometry = PointGeometry::from(arr);
		assert_eq!(point.x, 1.0);
		assert_eq!(point.y, 2.0);
	}

	#[test]
	fn test_point_geometry_from_f64_array() {
		let arr = [1.0, 2.0];
		let point: PointGeometry = PointGeometry::from(arr);
		assert_eq!(point.x, 1.0);
		assert_eq!(point.y, 2.0);
	}

	#[test]
	fn test_point_geometry_from_i64_array() {
		let arr = [1, 2];
		let point: PointGeometry = PointGeometry::from(arr);
		assert_eq!(point.x, 1.0);
		assert_eq!(point.y, 2.0);
	}
}
