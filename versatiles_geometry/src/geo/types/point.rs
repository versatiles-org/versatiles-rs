use super::{Coordinates, GeometryTrait, MultiPointGeometry, traits};
use std::fmt::Debug;
use traits::SingleGeometryTrait;
use versatiles_core::json::JsonValue;

/// Represents a single geographic or geometric point defined by x and y coordinates.
///
/// This is the simplest geometric type and is often used as a building block for more complex geometries.
#[derive(Clone, PartialEq)]
pub struct PointGeometry(pub Coordinates);

impl PointGeometry {
	/// Constructs a new `PointGeometry` from a `Coordinates` instance.
	#[must_use]
	pub fn new(c: Coordinates) -> Self {
		Self(c)
	}
	/// Returns the x component of the point.
	#[must_use]
	pub fn x(&self) -> f64 {
		self.0.x()
	}
	/// Returns the y component of the point.
	#[must_use]
	pub fn y(&self) -> f64 {
		self.0.y()
	}
	/// Returns a reference to the underlying `Coordinates`.
	#[must_use]
	pub fn as_coord(&self) -> &Coordinates {
		&self.0
	}
}

impl GeometryTrait for PointGeometry {
	/// Returns the area of the point, which is always 0 because points have no area.
	fn area(&self) -> f64 {
		0.0
	}

	/// Verifies the validity of the point.
	/// Always succeeds because a point is always valid.
	fn verify(&self) -> anyhow::Result<()> {
		Ok(())
	}

	/// Returns the point as a JSON array `[x, y]`, optionally rounded to the given precision.
	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		self.0.to_json(precision)
	}

	/// Points cannot contain other points, so this always returns `false`.
	fn contains_point(&self, _x: f64, _y: f64) -> bool {
		false
	}

	fn to_mercator(&self) -> PointGeometry {
		PointGeometry(self.0.to_mercator())
	}

	fn compute_bounds(&self) -> Option<[f64; 4]> {
		Some([self.0.x(), self.0.y(), self.0.x(), self.0.y()])
	}
}

impl SingleGeometryTrait<MultiPointGeometry> for PointGeometry {
	/// Wraps this single point into a `MultiPointGeometry`.
	fn into_multi(self) -> MultiPointGeometry {
		MultiPointGeometry(vec![self])
	}
}

impl Debug for PointGeometry {
	/// Formats the point as `[x, y]` for readability.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.0.fmt(f)
	}
}

impl<T> From<T> for PointGeometry
where
	Coordinates: From<T>,
{
	/// Allows creating a `PointGeometry` from any type convertible into `Coordinates`, such as arrays or tuples.
	fn from(value: T) -> Self {
		Self(Coordinates::from(value))
	}
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;

	#[test]
	fn new_and_accessors() {
		let point = PointGeometry::new(Coordinates::new(1.0, 2.0));
		assert_eq!(point.x(), 1.0);
		assert_eq!(point.y(), 2.0);
		assert_eq!(point.as_coord(), &Coordinates::new(1.0, 2.0));
	}

	#[test]
	fn eq_and_ne() {
		let p1 = PointGeometry::from(&[1, 2]);
		let p2 = PointGeometry::from(&[1, 2]);
		let p3 = PointGeometry::from(&[3, 4]);
		assert_eq!(p1, p2);
		assert_ne!(p1, p3);
	}

	#[test]
	fn debug_format() {
		assert_eq!(format!("{:?}", PointGeometry::from(&[1, 2])), "[1.0, 2.0]");
	}

	#[test]
	fn from_array_ref() {
		let p = PointGeometry::from(&[1, 2]);
		assert_eq!(p.x(), 1.0);
		assert_eq!(p.y(), 2.0);
	}

	#[test]
	fn from_f64_array() {
		let p = PointGeometry::from([1.0, 2.0]);
		assert_eq!(p.x(), 1.0);
		assert_eq!(p.y(), 2.0);
	}

	#[test]
	fn area_is_zero() {
		assert_eq!(PointGeometry::from(&[5, 10]).area(), 0.0);
	}

	#[test]
	fn verify_always_ok() {
		assert!(PointGeometry::from(&[0, 0]).verify().is_ok());
	}

	#[test]
	fn to_coord_json() {
		let json = PointGeometry::from([1.5, 2.5]).to_coord_json(None);
		assert_eq!(json, JsonValue::from([1.5, 2.5]));
	}

	#[test]
	fn to_coord_json_with_precision() {
		let json = PointGeometry::from([1.23456, 2.34567]).to_coord_json(Some(2));
		assert_eq!(json, JsonValue::from([1.23, 2.35]));
	}

	#[test]
	fn contains_point_is_always_false() {
		let p = PointGeometry::from(&[5, 5]);
		assert!(!p.contains_point(5.0, 5.0));
		assert!(!p.contains_point(0.0, 0.0));
	}

	#[test]
	fn to_mercator() {
		let p = PointGeometry::from([13.4, 52.5]);
		let m = p.to_mercator();
		assert!(m.x() > 0.0);
		assert!(m.y() > 0.0);
	}

	#[test]
	fn compute_bounds() {
		let bounds = PointGeometry::from([3.0, 7.0]).compute_bounds().unwrap();
		assert_eq!(bounds, [3.0, 7.0, 3.0, 7.0]);
	}

	#[test]
	fn into_multi() {
		use traits::CompositeGeometryTrait;
		let p = PointGeometry::from(&[1, 2]);
		let multi = p.clone().into_multi();
		assert_eq!(multi.as_vec().len(), 1);
		assert_eq!(multi.as_vec()[0], p);
	}

	#[test]
	fn clone() {
		let p = PointGeometry::from(&[1, 2]);
		assert_eq!(p.clone(), p);
	}
}
