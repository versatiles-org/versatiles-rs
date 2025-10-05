use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct Coordinates([f64; 2]);

impl Coordinates {
	pub fn new(x: f64, y: f64) -> Self {
		Self([x, y])
	}

	pub fn x(&self) -> f64 {
		self.0[0]
	}

	pub fn y(&self) -> f64 {
		self.0[1]
	}
}

impl<'a, T> From<&'a [T; 2]> for Coordinates
where
	T: Copy + Into<f64>,
{
	fn from(value: &'a [T; 2]) -> Self {
		Coordinates([value[0].into(), value[1].into()])
	}
}

impl From<[f64; 2]> for Coordinates {
	fn from(value: [f64; 2]) -> Self {
		Coordinates(value)
	}
}

impl<T> From<Coordinates> for [T; 2]
where
	T: From<f64>,
{
	fn from(value: Coordinates) -> Self {
		[T::from(value.0[0]), T::from(value.0[1])]
	}
}

impl From<geo::Coord> for Coordinates {
	fn from(value: geo::Coord) -> Self {
		Coordinates([value.x, value.y])
	}
}

impl Debug for Coordinates {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.0.fmt(f)
	}
}
