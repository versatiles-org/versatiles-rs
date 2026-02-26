use super::{CompositeGeometryTrait, GeometryTrait, LineStringGeometry};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

/// Represents a collection of connected line strings, each being a sequence of coordinates.
/// Typically used for multi-part paths or networks in 2D space.
#[derive(Clone, PartialEq)]
pub struct MultiLineStringGeometry(pub Vec<LineStringGeometry>);

/// Implementation of the `GeometryTrait` for `MultiLineStringGeometry`.
impl GeometryTrait for MultiLineStringGeometry {
	/// Returns the area of the geometry, which is always 0 for line strings since they have no area.
	fn area(&self) -> f64 {
		0.0
	}

	/// Verifies that all inner `LineStringGeometry` elements are valid.
	fn verify(&self) -> Result<()> {
		for line in &self.0 {
			line.verify()?;
		}
		Ok(())
	}

	/// Converts the geometry into a JSON representation, optionally rounding coordinates to the given precision.
	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		JsonValue::from(
			self
				.0
				.iter()
				.map(|line| line.to_coord_json(precision))
				.collect::<Vec<_>>(),
		)
	}

	/// Lines cannot contain points, so this always returns `false`.
	fn contains_point(&self, _x: f64, _y: f64) -> bool {
		false
	}

	fn to_mercator(&self) -> MultiLineStringGeometry {
		MultiLineStringGeometry(self.0.iter().map(LineStringGeometry::to_mercator).collect())
	}

	fn compute_bounds(&self) -> Option<[f64; 4]> {
		let mut x_min = f64::MAX;
		let mut y_min = f64::MAX;
		let mut x_max = f64::MIN;
		let mut y_max = f64::MIN;
		let mut has_bounds = false;

		for line in &self.0 {
			if let Some(bounds) = line.compute_bounds() {
				has_bounds = true;
				x_min = x_min.min(bounds[0]);
				y_min = y_min.min(bounds[1]);
				x_max = x_max.max(bounds[2]);
				y_max = y_max.max(bounds[3]);
			}
		}

		if has_bounds {
			Some([x_min, y_min, x_max, y_max])
		} else {
			None
		}
	}
}

/// Provides methods to work with the internal vector of `LineStringGeometry` objects.
impl CompositeGeometryTrait<LineStringGeometry> for MultiLineStringGeometry {
	/// Creates an empty `MultiLineStringGeometry`.
	fn new() -> Self {
		Self(Vec::new())
	}
	/// Returns an immutable reference to the internal vector of `LineStringGeometry`.
	fn as_vec(&self) -> &Vec<LineStringGeometry> {
		&self.0
	}
	/// Returns a mutable reference to the internal vector of `LineStringGeometry`.
	fn as_mut_vec(&mut self) -> &mut Vec<LineStringGeometry> {
		&mut self.0
	}
	/// Consumes the geometry and returns the internal vector of `LineStringGeometry`.
	fn into_inner(self) -> Vec<LineStringGeometry> {
		self.0
	}
}

/// Implements the `Debug` trait to print the collection of line strings in a developer-friendly format.
impl Debug for MultiLineStringGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(MultiLineStringGeometry, LineStringGeometry);

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;

	fn sample() -> MultiLineStringGeometry {
		MultiLineStringGeometry::from(vec![
			vec![(0.0, 0.0), (1.0, 1.0)],
			vec![(2.0, 3.0), (4.0, 5.0), (6.0, 7.0)],
		])
	}

	// ── GeometryTrait ───────────────────────────────────────────────────

	#[test]
	fn area_is_zero() {
		assert_eq!(sample().area(), 0.0);
	}

	#[test]
	fn verify_valid() {
		assert!(sample().verify().is_ok());
	}

	#[test]
	fn verify_empty_ok() {
		assert!(MultiLineStringGeometry::new().verify().is_ok());
	}

	#[test]
	fn verify_invalid_inner() {
		// A line with only 1 point is invalid
		let mut ml = MultiLineStringGeometry::new();
		ml.push(LineStringGeometry::from(vec![(0.0, 0.0)]));
		assert!(ml.verify().is_err());
	}

	#[test]
	fn to_coord_json() {
		let json = sample().to_coord_json(None);
		let arr = json.as_array().unwrap();
		assert_eq!(arr.len(), 2);
	}

	#[test]
	fn contains_point_always_false() {
		assert!(!sample().contains_point(0.0, 0.0));
		assert!(!sample().contains_point(5.0, 5.0));
	}

	#[test]
	fn to_mercator() {
		let ml = MultiLineStringGeometry::from(&[[(13.0, 52.0), (13.5, 52.5)]]);
		let m = ml.to_mercator();
		assert_eq!(m.as_vec().len(), 1);
		assert!(m.as_vec()[0].as_vec()[0].x() > 0.0);
	}

	#[test]
	fn compute_bounds() {
		let bounds = sample().compute_bounds().unwrap();
		assert_eq!(bounds, [0.0, 0.0, 6.0, 7.0]);
	}

	#[test]
	fn compute_bounds_empty() {
		assert!(MultiLineStringGeometry::new().compute_bounds().is_none());
	}

	// ── CompositeGeometryTrait ──────────────────────────────────────────

	#[test]
	fn composite_new_is_empty() {
		let ml = MultiLineStringGeometry::new();
		assert!(ml.is_empty());
		assert_eq!(ml.len(), 0);
	}

	#[test]
	fn composite_push_and_len() {
		let mut ml = MultiLineStringGeometry::new();
		ml.push(LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]));
		assert_eq!(ml.len(), 1);
		assert!(!ml.is_empty());
	}

	#[test]
	fn composite_first_last() {
		let ml = sample();
		assert_eq!(ml.first().unwrap().as_vec()[0].x(), 0.0);
		assert_eq!(ml.last().unwrap().as_vec()[0].x(), 2.0);
	}

	#[test]
	fn composite_first_last_empty() {
		let ml = MultiLineStringGeometry::new();
		assert!(ml.first().is_none());
		assert!(ml.last().is_none());
	}

	#[test]
	fn composite_pop() {
		let mut ml = sample();
		let popped = ml.pop().unwrap();
		assert_eq!(popped.len(), 3);
		assert_eq!(ml.len(), 1);
	}

	#[test]
	fn composite_into_inner() {
		let inner = sample().into_inner();
		assert_eq!(inner.len(), 2);
	}

	#[test]
	fn composite_into_iter() {
		let lines: Vec<_> = sample().into_iter().collect();
		assert_eq!(lines.len(), 2);
	}

	#[test]
	fn composite_into_first_and_rest() {
		let (first, rest) = sample().into_first_and_rest().unwrap();
		assert_eq!(first.len(), 2);
		assert_eq!(rest.len(), 1);
	}

	#[test]
	fn composite_into_first_and_rest_empty() {
		assert!(MultiLineStringGeometry::new().into_first_and_rest().is_none());
	}

	// ── Debug / Clone / Eq ──────────────────────────────────────────────

	#[test]
	fn debug_format() {
		let ml = MultiLineStringGeometry::from(&[[(1.0, 2.0), (3.0, 4.0)]]);
		assert!(format!("{ml:?}").contains("[1.0, 2.0]"));
	}

	#[test]
	fn clone_and_eq() {
		let a = sample();
		assert_eq!(a.clone(), a);
	}

	#[test]
	fn ne() {
		let a = MultiLineStringGeometry::from(&[[(0.0, 0.0), (1.0, 1.0)]]);
		let b = MultiLineStringGeometry::from(&[[(2.0, 2.0), (3.0, 3.0)]]);
		assert_ne!(a, b);
	}

	// ── From conversions ────────────────────────────────────────────────

	#[test]
	fn from_vec() {
		let ml = MultiLineStringGeometry::from(vec![vec![(0.0, 0.0), (1.0, 1.0)], vec![(2.0, 2.0), (3.0, 3.0)]]);
		assert_eq!(ml.len(), 2);
	}

	#[test]
	fn from_slice() {
		let data = [[(0.0, 0.0), (1.0, 1.0)], [(2.0, 2.0), (3.0, 3.0)]];
		let ml = MultiLineStringGeometry::from(&data[..]);
		assert_eq!(ml.len(), 2);
	}
}
