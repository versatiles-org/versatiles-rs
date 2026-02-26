use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};
use std::fmt::Debug;

use versatiles_core::json::JsonValue;
use versatiles_core::{EARTH_RADIUS, MAX_LAT};

/// A simple 2D coordinate pair `(x, y)`.
///
/// This struct is used to represent points in 2D space for geometric and geospatial data.
#[derive(Clone, PartialEq)]
pub struct Coordinates([f64; 2]);

impl Coordinates {
	/// Constructs a new `Coordinates` instance with the given `x` and `y` values.
	#[must_use]
	pub fn new(x: f64, y: f64) -> Self {
		Self([x, y])
	}

	/// Returns the `x` component of the coordinate.
	#[must_use]
	pub fn x(&self) -> f64 {
		self.0[0]
	}

	/// Returns the `y` component of the coordinate.
	#[must_use]
	pub fn y(&self) -> f64 {
		self.0[1]
	}

	/// Returns the coordinates as a JSON array.
	///
	/// If `precision` is specified, the coordinate values will be rounded to the given number of decimal places.
	#[must_use]
	pub fn to_json(&self, precision: Option<u8>) -> JsonValue {
		if let Some(prec) = precision {
			let factor = 10f64.powi(i32::from(prec));
			let x = (self.0[0] * factor).round() / factor;
			let y = (self.0[1] * factor).round() / factor;
			JsonValue::from([x, y])
		} else {
			JsonValue::from(&self.0)
		}
	}

	/// Convert WGS84 coordinates (longitude, latitude) to Web Mercator (x, y) in meters.
	///
	/// Interprets this coordinate as (longitude, latitude) in degrees and returns
	/// the equivalent Web Mercator (EPSG:3857) coordinates in meters.
	///
	/// # Note
	/// Latitude is clamped to ±85.051129 degrees, the valid range for Web Mercator.
	#[must_use]
	pub fn to_mercator(&self) -> Coordinates {
		let lon = self.x();
		let lat = self.y().clamp(-MAX_LAT, MAX_LAT);

		let x = lon.to_radians() * EARTH_RADIUS;
		let y = (lat.to_radians() / 2.0 + FRAC_PI_4).tan().ln() * EARTH_RADIUS;
		Coordinates::new(x, y)
	}

	/// Convert Web Mercator coordinates (x, y) in meters to WGS84 (longitude, latitude) in degrees.
	///
	/// Interprets this coordinate as Web Mercator (EPSG:3857) in meters and returns
	/// the equivalent WGS84 coordinates (longitude, latitude) in degrees.
	#[must_use]
	pub fn from_mercator(&self) -> Coordinates {
		let x = self.x();
		let y = self.y();

		let lon = (x / EARTH_RADIUS).to_degrees();
		let lat = (2.0 * (y / EARTH_RADIUS).exp().atan() - FRAC_PI_2).to_degrees();
		Coordinates::new(lon, lat)
	}
}

/// Converts from a reference to an array of two elements into `Coordinates`.
impl<'a, T> From<&'a [T; 2]> for Coordinates
where
	T: Copy + Into<f64>,
{
	fn from(value: &'a [T; 2]) -> Self {
		Coordinates([value[0].into(), value[1].into()])
	}
}

/// Converts from a `[f64; 2]` array into `Coordinates`.
impl From<[f64; 2]> for Coordinates {
	fn from(value: [f64; 2]) -> Self {
		Coordinates(value)
	}
}

/// Converts from a tuple `(f64, f64)` into `Coordinates`.
impl From<(f64, f64)> for Coordinates {
	fn from(value: (f64, f64)) -> Self {
		Coordinates([value.0, value.1])
	}
}

/// Converts from a reference to a tuple `(f64, f64)` into `Coordinates`.
impl From<&(f64, f64)> for Coordinates {
	fn from(value: &(f64, f64)) -> Self {
		Coordinates([value.0, value.1])
	}
}

/// Converts from `Coordinates` into a `[f64; 2]` array.
impl From<Coordinates> for [f64; 2] {
	fn from(value: Coordinates) -> Self {
		[value.0[0], value.0[1]]
	}
}

/// Converts from a `geo::Coord` into `Coordinates`.
impl From<geo::Coord> for Coordinates {
	fn from(value: geo::Coord) -> Self {
		Coordinates([value.x, value.y])
	}
}

/// Implements the `Debug` trait for `Coordinates`.
///
/// The coordinates are printed in the format `[x, y]`.
impl Debug for Coordinates {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.0.fmt(f)
	}
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
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
		assert_eq!(format!("{c:?}"), "[1.0, 2.0]");
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

	#[test]
	fn to_mercator_origin() {
		let wgs84 = Coordinates::new(0.0, 0.0);
		let mercator = wgs84.to_mercator();
		assert!((mercator.x() - 0.0).abs() < 1e-6);
		assert!((mercator.y() - 0.0).abs() < 1e-6);
	}

	#[test]
	fn to_mercator_positive_coords() {
		// Berlin approximate
		let wgs84 = Coordinates::new(13.4, 52.5);
		let mercator = wgs84.to_mercator();
		assert!(mercator.x() > 0.0);
		assert!(mercator.y() > 0.0);
	}

	#[test]
	fn to_mercator_negative_coords() {
		// New York approximate
		let wgs84 = Coordinates::new(-74.0, 40.7);
		let mercator = wgs84.to_mercator();
		assert!(mercator.x() < 0.0);
		assert!(mercator.y() > 0.0);
	}

	#[test]
	fn mercator_roundtrip() {
		let original = Coordinates::new(10.0, 50.0);
		let mercator = original.to_mercator();
		let back = mercator.from_mercator();
		assert!((original.x() - back.x()).abs() < 1e-6);
		assert!((original.y() - back.y()).abs() < 1e-6);
	}

	#[test]
	fn mercator_latitude_clamping() {
		// Latitude beyond max should be clamped
		let wgs84 = Coordinates::new(0.0, 90.0);
		let mercator = wgs84.to_mercator();
		// Should not be infinite
		assert!(mercator.y().is_finite());
	}
}
