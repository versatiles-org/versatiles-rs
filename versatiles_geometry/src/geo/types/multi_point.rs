use super::{CompositeGeometryTrait, GeometryTrait, PointGeometry};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

/// Represents a collection of points, used to store multiple discrete locations in 2D space.
#[derive(Clone, PartialEq)]
pub struct MultiPointGeometry(pub Vec<PointGeometry>);

/// Implementation of `GeometryTrait` for `MultiPointGeometry`.
///
/// - `area()` returns 0 because points have no area.
/// - `verify()` checks that all contained points are valid.
/// - `to_coord_json()` converts the geometry into a JSON array of coordinates,
///   optionally rounding to a given precision.
impl GeometryTrait for MultiPointGeometry {
	fn area(&self) -> f64 {
		0.0
	}

	fn verify(&self) -> Result<()> {
		for point in &self.0 {
			point.verify()?;
		}
		Ok(())
	}

	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		JsonValue::from(
			self
				.0
				.iter()
				.map(|point| point.to_coord_json(precision))
				.collect::<Vec<_>>(),
		)
	}

	/// Points cannot contain other points, so this always returns `false`.
	fn contains_point(&self, _x: f64, _y: f64) -> bool {
		false
	}

	fn to_mercator(&self) -> MultiPointGeometry {
		MultiPointGeometry(self.0.iter().map(PointGeometry::to_mercator).collect())
	}

	fn compute_bounds(&self) -> Option<[f64; 4]> {
		if self.0.is_empty() {
			return None;
		}

		let mut x_min = f64::MAX;
		let mut y_min = f64::MAX;
		let mut x_max = f64::MIN;
		let mut y_max = f64::MIN;

		for point in &self.0 {
			// Points always have bounds, so unwrap is safe here
			let bounds = point.compute_bounds().unwrap();
			x_min = x_min.min(bounds[0]);
			y_min = y_min.min(bounds[1]);
			x_max = x_max.max(bounds[2]);
			y_max = y_max.max(bounds[3]);
		}

		Some([x_min, y_min, x_max, y_max])
	}
}

/// Provides methods to access and manage the internal vector of points for `MultiPointGeometry`.
impl CompositeGeometryTrait<PointGeometry> for MultiPointGeometry {
	/// Creates a new, empty `MultiPointGeometry`.
	fn new() -> Self {
		Self(Vec::new())
	}
	/// Returns an immutable reference to the internal vector of points.
	fn as_vec(&self) -> &Vec<PointGeometry> {
		&self.0
	}
	/// Returns a mutable reference to the internal vector of points.
	fn as_mut_vec(&mut self) -> &mut Vec<PointGeometry> {
		&mut self.0
	}
	/// Consumes self and returns the internal vector of points.
	fn into_inner(self) -> Vec<PointGeometry> {
		self.0
	}
}

/// Implements the `Debug` trait to print the list of contained points in a developer-friendly format.
impl Debug for MultiPointGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(MultiPointGeometry, PointGeometry);

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;

	fn sample() -> MultiPointGeometry {
		MultiPointGeometry::from(&[[1, 2], [3, 4], [5, 6]])
	}

	// ── GeometryTrait ───────────────────────────────────────────────────

	#[test]
	fn area_is_zero() {
		assert_eq!(sample().area(), 0.0);
	}

	#[test]
	fn verify_ok() {
		assert!(sample().verify().is_ok());
	}

	#[test]
	fn verify_empty_ok() {
		assert!(MultiPointGeometry::new().verify().is_ok());
	}

	#[test]
	fn to_coord_json() {
		let json = sample().to_coord_json(None);
		let arr = json.as_array().unwrap();
		assert_eq!(arr.len(), 3);
	}

	#[test]
	fn to_coord_json_with_precision() {
		let mp = MultiPointGeometry::from(vec![(1.23456, 2.34567)]);
		let json = mp.to_coord_json(Some(2));
		let arr = json.as_array().unwrap();
		assert_eq!(arr.len(), 1);
	}

	#[test]
	fn contains_point_always_false() {
		assert!(!sample().contains_point(1.0, 2.0));
		assert!(!sample().contains_point(0.0, 0.0));
	}

	#[test]
	fn to_mercator() {
		let mp = MultiPointGeometry::from(&[[13, 52], [-74, 40]]);
		let m = mp.to_mercator();
		assert_eq!(m.as_vec().len(), 2);
		assert!(m.as_vec()[0].x() > 0.0); // Berlin-ish
		assert!(m.as_vec()[1].x() < 0.0); // New York-ish
	}

	#[test]
	fn compute_bounds() {
		let bounds = sample().compute_bounds().unwrap();
		assert_eq!(bounds, [1.0, 2.0, 5.0, 6.0]);
	}

	#[test]
	fn compute_bounds_single_point() {
		let mp = MultiPointGeometry::from(&[[7, 8]]);
		let bounds = mp.compute_bounds().unwrap();
		assert_eq!(bounds, [7.0, 8.0, 7.0, 8.0]);
	}

	#[test]
	fn compute_bounds_empty() {
		assert!(MultiPointGeometry::new().compute_bounds().is_none());
	}

	// ── CompositeGeometryTrait ──────────────────────────────────────────

	#[test]
	fn composite_new_is_empty() {
		let mp = MultiPointGeometry::new();
		assert!(mp.is_empty());
		assert_eq!(mp.len(), 0);
	}

	#[test]
	fn composite_push_and_len() {
		let mut mp = MultiPointGeometry::new();
		mp.push(PointGeometry::from(&[1, 2]));
		mp.push(PointGeometry::from(&[3, 4]));
		assert_eq!(mp.len(), 2);
		assert!(!mp.is_empty());
	}

	#[test]
	fn composite_first_last() {
		let mp = sample();
		assert_eq!(mp.first().unwrap().x(), 1.0);
		assert_eq!(mp.last().unwrap().x(), 5.0);
	}

	#[test]
	fn composite_first_last_empty() {
		let mp = MultiPointGeometry::new();
		assert!(mp.first().is_none());
		assert!(mp.last().is_none());
	}

	#[test]
	fn composite_pop() {
		let mut mp = sample();
		let popped = mp.pop().unwrap();
		assert_eq!(popped.x(), 5.0);
		assert_eq!(mp.len(), 2);
	}

	#[test]
	fn composite_into_inner() {
		let inner = sample().into_inner();
		assert_eq!(inner.len(), 3);
	}

	#[test]
	fn composite_into_iter() {
		let points: Vec<_> = sample().into_iter().collect();
		assert_eq!(points.len(), 3);
	}

	#[test]
	fn composite_into_first_and_rest() {
		let (first, rest) = sample().into_first_and_rest().unwrap();
		assert_eq!(first.x(), 1.0);
		assert_eq!(rest.len(), 2);
	}

	#[test]
	fn composite_into_first_and_rest_empty() {
		assert!(MultiPointGeometry::new().into_first_and_rest().is_none());
	}

	// ── Debug / Clone / Eq ──────────────────────────────────────────────

	#[test]
	fn debug_format() {
		let mp = MultiPointGeometry::from(&[[1, 2]]);
		assert!(format!("{mp:?}").contains("[1.0, 2.0]"));
	}

	#[test]
	fn clone_and_eq() {
		let a = sample();
		assert_eq!(a.clone(), a);
	}

	#[test]
	fn ne() {
		let a = MultiPointGeometry::from(&[[1, 2]]);
		let b = MultiPointGeometry::from(&[[3, 4]]);
		assert_ne!(a, b);
	}

	// ── From conversions ────────────────────────────────────────────────

	#[test]
	fn from_vec() {
		let mp = MultiPointGeometry::from(vec![(1.0, 2.0), (3.0, 4.0)]);
		assert_eq!(mp.len(), 2);
	}

	#[test]
	fn from_slice() {
		let data = [(1.0, 2.0), (3.0, 4.0)];
		let mp = MultiPointGeometry::from(&data[..]);
		assert_eq!(mp.len(), 2);
	}
}
