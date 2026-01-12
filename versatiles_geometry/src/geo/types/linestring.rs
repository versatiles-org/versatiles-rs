use super::{CompositeGeometryTrait, Coordinates, GeometryTrait, MultiLineStringGeometry, SingleGeometryTrait};
use anyhow::{Result, ensure};
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

/// Represents a sequence of connected coordinates forming a line, typically used for polylines or paths in 2D space.
#[derive(Clone, PartialEq)]
pub struct LineStringGeometry(pub Vec<Coordinates>);

impl GeometryTrait for LineStringGeometry {
	/// Returns the area of the geometry.
	///
	/// For a line, this is always 0 because a line has no area.
	fn area(&self) -> f64 {
		0.0
	}

	/// Verifies the validity of the geometry.
	///
	/// Ensures that the `LineStringGeometry` has at least two points.
	fn verify(&self) -> Result<()> {
		ensure!(self.0.len() >= 2, "LineString must have at least two points");
		Ok(())
	}

	/// Converts the line's coordinates into a JSON representation.
	///
	/// Optionally rounds the coordinates to the specified precision.
	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		JsonValue::from(self.0.iter().map(|c| c.to_json(precision)).collect::<Vec<_>>())
	}
}

impl CompositeGeometryTrait<Coordinates> for LineStringGeometry {
	/// Creates a new, empty `LineStringGeometry`.
	fn new() -> Self {
		Self(Vec::new())
	}
	/// Returns a reference to the internal vector of coordinates representing the points of the line.
	fn as_vec(&self) -> &Vec<Coordinates> {
		&self.0
	}
	/// Returns a mutable reference to the internal vector of coordinates representing the points of the line.
	fn as_mut_vec(&mut self) -> &mut Vec<Coordinates> {
		&mut self.0
	}

	/// Consumes the `LineStringGeometry` and returns the internal vector of coordinates.
	fn into_inner(self) -> Vec<Coordinates> {
		self.0
	}
}

impl SingleGeometryTrait<MultiLineStringGeometry> for LineStringGeometry {
	/// Converts this single line into a `MultiLineStringGeometry` containing just this one line.
	fn into_multi(self) -> MultiLineStringGeometry {
		MultiLineStringGeometry(vec![self])
	}
}

impl Debug for LineStringGeometry {
	/// Formats the `LineStringGeometry` using the given formatter.
	///
	/// Prints the list of coordinates in a developer-friendly format.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(LineStringGeometry, Coordinates);

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;

	// Tests for impl_from_array! macro-generated From implementations

	#[test]
	fn test_from_vec() {
		let coords: Vec<(f64, f64)> = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)];
		let line = LineStringGeometry::from(coords);
		assert_eq!(line.0.len(), 3);
		assert_eq!(line.0[0].x(), 0.0);
		assert_eq!(line.0[2].x(), 2.0);
	}

	#[test]
	fn test_from_vec_ref() {
		let coords: Vec<(f64, f64)> = vec![(0.0, 0.0), (1.0, 1.0)];
		let line = LineStringGeometry::from(&coords);
		assert_eq!(line.0.len(), 2);
	}

	#[test]
	fn test_from_slice() {
		let coords: [(f64, f64); 3] = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)];
		let line = LineStringGeometry::from(&coords[..]);
		assert_eq!(line.0.len(), 3);
	}

	#[test]
	fn test_from_array_ref() {
		let coords: [(f64, f64); 2] = [(0.0, 0.0), (1.0, 1.0)];
		let line = LineStringGeometry::from(&coords);
		assert_eq!(line.0.len(), 2);
	}

	// Tests for LineStringGeometry methods

	#[test]
	fn test_area() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		assert_eq!(line.area(), 0.0);
	}

	#[test]
	fn test_verify_valid() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		assert!(line.verify().is_ok());
	}

	#[test]
	fn test_verify_invalid() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0)]);
		assert!(line.verify().is_err());
	}

	#[test]
	fn test_to_coord_json() {
		let line = LineStringGeometry::from(vec![(1.0, 2.0), (3.0, 4.0)]);
		let json = line.to_coord_json(None);
		let arr = json.as_array().unwrap();
		assert_eq!(arr.len(), 2);
	}

	#[test]
	fn test_new_and_as_vec() {
		let line = LineStringGeometry::new();
		assert!(line.as_vec().is_empty());
	}

	#[test]
	fn test_as_mut_vec() {
		let mut line = LineStringGeometry::new();
		line.as_mut_vec().push(Coordinates::from((1.0, 2.0)));
		assert_eq!(line.0.len(), 1);
	}

	#[test]
	fn test_into_inner() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		let inner = line.into_inner();
		assert_eq!(inner.len(), 2);
	}

	#[test]
	fn test_into_multi() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		let multi = line.into_multi();
		assert_eq!(multi.as_vec().len(), 1);
	}

	#[test]
	fn test_debug() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		let debug_str = format!("{line:?}");
		assert!(debug_str.contains('['));
	}

	#[test]
	fn test_clone_and_eq() {
		let line1 = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		let line2 = line1.clone();
		assert_eq!(line1, line2);
	}

	// Tests for CompositeGeometryTrait default methods (from traits.rs)

	#[test]
	fn test_into_iter() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]);
		let coords: Vec<_> = line.into_iter().collect();
		assert_eq!(coords.len(), 3);
		assert_eq!(coords[0].x(), 0.0);
		assert_eq!(coords[2].x(), 2.0);
	}

	#[test]
	fn test_into_first_and_rest() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]);
		let (first, rest) = line.into_first_and_rest().unwrap();
		assert_eq!(first.x(), 0.0);
		assert_eq!(rest.len(), 2);
	}

	#[test]
	fn test_into_first_and_rest_empty() {
		let line = LineStringGeometry::new();
		assert!(line.into_first_and_rest().is_none());
	}

	#[test]
	fn test_is_empty() {
		let empty = LineStringGeometry::new();
		assert!(empty.is_empty());

		let non_empty = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		assert!(!non_empty.is_empty());
	}

	#[test]
	fn test_len() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]);
		assert_eq!(line.len(), 3);
	}

	#[test]
	fn test_push() {
		let mut line = LineStringGeometry::new();
		line.push(Coordinates::from((1.0, 2.0)));
		line.push(Coordinates::from((3.0, 4.0)));
		assert_eq!(line.len(), 2);
	}

	#[test]
	fn test_pop() {
		let mut line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		let popped = line.pop().unwrap();
		assert_eq!(popped.x(), 1.0);
		assert_eq!(line.len(), 1);
	}

	#[test]
	fn test_pop_empty() {
		let mut line = LineStringGeometry::new();
		assert!(line.pop().is_none());
	}

	#[test]
	fn test_first_and_last() {
		let line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]);
		assert_eq!(line.first().unwrap().x(), 0.0);
		assert_eq!(line.last().unwrap().x(), 2.0);
	}

	#[test]
	fn test_first_and_last_empty() {
		let line = LineStringGeometry::new();
		assert!(line.first().is_none());
		assert!(line.last().is_none());
	}

	#[test]
	fn test_first_mut() {
		let mut line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		if let Some(first) = line.first_mut() {
			*first = Coordinates::from((9.0, 9.0));
		}
		assert_eq!(line.first().unwrap().x(), 9.0);
	}

	#[test]
	fn test_last_mut() {
		let mut line = LineStringGeometry::from(vec![(0.0, 0.0), (1.0, 1.0)]);
		if let Some(last) = line.last_mut() {
			*last = Coordinates::from((9.0, 9.0));
		}
		assert_eq!(line.last().unwrap().x(), 9.0);
	}
}
