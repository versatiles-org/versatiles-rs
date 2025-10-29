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

impl From<Coordinates> for [f64; 2] {
	fn from(value: Coordinates) -> Self {
		[value.0[0], value.0[1]]
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

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[test]
	fn new_and_accessors() {
		let c = Coordinates::new(13.404954, 52.520008);
		assert_eq!(c.x(), 13.404954);
		assert_eq!(c.y(), 52.520008);
	}

	#[test]
	fn debug_formats_like_array() {
		let c = Coordinates::new(1.0, 2.0);
		assert_eq!(format!("{:?}", c), "[1.0, 2.0]");
	}

	#[test]
	fn to_json_without_precision() {
		let c = Coordinates::new(1.23456789, 9.87654321);
		let json = c.to_json(None);
		assert_eq!(json, JsonValue::from([1.23456789, 9.87654321]));
	}

	#[rstest]
	#[case(0, [1.0, 2.0])]
	#[case(1, [1.2, 2.3])]
	#[case(3, [1.235, 2.346])]
	fn to_json_with_precision(#[case] prec: u8, #[case] expected: [f64; 2]) {
		let c = Coordinates::new(1.23456, 2.34567);
		let json = c.to_json(Some(prec));
		assert_eq!(json, JsonValue::from(expected));
	}

	#[test]
	fn from_array_ref() {
		let a = [7.0f64, 8.0f64];
		let c = Coordinates::from(&a);
		assert_eq!(c.x(), 7.0);
		assert_eq!(c.y(), 8.0);
	}

	#[test]
	fn from_tuple_and_ref_tuple() {
		let c1 = Coordinates::from((3.0f64, 4.0f64));
		let t = (5.0f64, 6.0f64);
		let c2 = Coordinates::from(&t);
		assert_eq!(c1.x(), 3.0);
		assert_eq!(c1.y(), 4.0);
		assert_eq!(c2.x(), 5.0);
		assert_eq!(c2.y(), 6.0);
	}

	#[test]
	fn into_array_f64_and_f32() {
		let c = Coordinates::new(10.25, -20.5);
		let arr_f64: [f64; 2] = c.into();
		assert_eq!(arr_f64, [10.25, -20.5]);
	}

	#[test]
	fn from_geo_coord() {
		let gc = geo::Coord { x: 11.0, y: 22.0 };
		let c = Coordinates::from(gc);
		assert_eq!(c.x(), 11.0);
		assert_eq!(c.y(), 22.0);
	}

	#[test]
	fn clone_and_eq() {
		let a = Coordinates::new(1.0, 2.0);
		let b = a.clone();
		assert_eq!(a, b);
	}
}
