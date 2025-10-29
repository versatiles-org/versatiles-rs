use std::fmt::Debug;

use versatiles_core::json::JsonValue;

#[derive(Clone, PartialEq)]
pub struct Coordinates([f64; 2]);

impl Coordinates {
	#[must_use]
	pub fn new(x: f64, y: f64) -> Self {
		Self([x, y])
	}

	#[must_use]
	pub fn x(&self) -> f64 {
		self.0[0]
	}

	#[must_use]
	pub fn y(&self) -> f64 {
		self.0[1]
	}

	#[must_use]
	pub fn to_json(&self, precision: Option<u8>) -> JsonValue {
		if let Some(prec) = precision {
			let factor = 10f64.powi(prec as i32);
			let x = (self.0[0] * factor).round() / factor;
			let y = (self.0[1] * factor).round() / factor;
			JsonValue::from([x, y])
		} else {
			JsonValue::from(&self.0)
		}
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

impl From<(f64, f64)> for Coordinates {
	fn from(value: (f64, f64)) -> Self {
		Coordinates([value.0, value.1])
	}
}

impl From<&(f64, f64)> for Coordinates {
	fn from(value: &(f64, f64)) -> Self {
		Coordinates([value.0, value.1])
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
