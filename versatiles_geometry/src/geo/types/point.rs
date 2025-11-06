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
mod tests {
	use super::*;

	#[test]
	fn test_point_geometry_new() {
		let point = PointGeometry::from(&[1, 2]);
		assert_eq!(point.x(), 1.0);
		assert_eq!(point.y(), 2.0);
	}

	#[test]
	fn test_point_geometry_eq() {
		let point1 = PointGeometry::from(&[1, 2]);
		let point2 = PointGeometry::from(&[1, 2]);
		let point3 = PointGeometry::from(&[3, 4]);
		assert_eq!(point1, point2);
		assert_ne!(point1, point3);
	}

	#[test]
	fn test_point_geometry_debug() {
		let point = PointGeometry::from(&[1, 2]);
		assert_eq!(format!("{point:?}"), "[1.0, 2.0]");
	}

	#[test]
	fn test_point_geometry_from_f64_array_ref() {
		let arr = &[1, 2];
		let point: PointGeometry = PointGeometry::from(arr);
		assert_eq!(point.x(), 1.0);
		assert_eq!(point.y(), 2.0);
	}

	#[test]
	fn test_point_geometry_from_f64_array() {
		let arr = [1.0, 2.0];
		let point: PointGeometry = PointGeometry::from(arr);
		assert_eq!(point.x(), 1.0);
		assert_eq!(point.y(), 2.0);
	}

	#[test]
	fn test_point_geometry_from_i64_array() {
		let arr = [1, 2];
		let point: PointGeometry = PointGeometry::from(&arr);
		assert_eq!(point.x(), 1.0);
		assert_eq!(point.y(), 2.0);
	}
}
