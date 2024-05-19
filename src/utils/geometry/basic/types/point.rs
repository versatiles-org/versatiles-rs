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
