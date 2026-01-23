use super::{CompositeGeometryTrait, Coordinates, GeometryTrait};
use anyhow::{Result, ensure};
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

/// Represents a closed ring geometry, which is a connected series of coordinates forming a loop.
/// This structure is typically used as the building block for polygons.
/// The first and last points must be identical to form a closed shape.
#[derive(Clone, PartialEq)]
pub struct RingGeometry(pub Vec<Coordinates>);

impl GeometryTrait for RingGeometry {
	/// Computes the signed area of the ring using the shoelace formula.
	/// The area is positive if the ring is oriented counterclockwise,
	/// and negative if clockwise.
	fn area(&self) -> f64 {
		let mut sum = 0f64;
		if let Some(mut p2) = self.0.last() {
			for p1 in &self.0 {
				sum += (p2.x() - p1.x()) * (p1.y() + p2.y());
				p2 = p1;
			}
		}
		sum
	}

	/// Verifies that the ring is valid by checking:
	/// - It has at least 4 coordinates (3 unique points plus the closing point).
	/// - It is closed, i.e., the first and last points are identical.
	fn verify(&self) -> Result<()> {
		ensure!(self.0.len() >= 4, "Ring must have at least 4 points");
		ensure!(self.0.first() == self.0.last(), "Ring must be closed");
		Ok(())
	}

	/// Returns the coordinates of the ring as a JSON array.
	/// If a precision is specified, coordinates are rounded accordingly.
	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		JsonValue::from(self.0.iter().map(|coord| coord.to_json(precision)).collect::<Vec<_>>())
	}

	fn contains_point(&self, x: f64, y: f64) -> bool {
		let coords = &self.0;
		if coords.len() < 4 {
			return false;
		}

		let mut inside = false;
		let mut j = coords.len() - 1;

		for i in 0..coords.len() {
			let xi = coords[i].x();
			let yi = coords[i].y();
			let xj = coords[j].x();
			let yj = coords[j].y();

			// Check if point is on the same side and crosses the ray
			if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
				inside = !inside;
			}
			j = i;
		}

		inside
	}

	fn to_mercator(&self) -> RingGeometry {
		RingGeometry(self.0.iter().map(Coordinates::to_mercator).collect())
	}

	fn compute_bounds(&self) -> Option<[f64; 4]> {
		if self.0.is_empty() {
			return None;
		}

		let mut x_min = f64::MAX;
		let mut y_min = f64::MAX;
		let mut x_max = f64::MIN;
		let mut y_max = f64::MIN;

		for coord in &self.0 {
			x_min = x_min.min(coord.x());
			y_min = y_min.min(coord.y());
			x_max = x_max.max(coord.x());
			y_max = y_max.max(coord.y());
		}

		Some([x_min, y_min, x_max, y_max])
	}
}

impl CompositeGeometryTrait<Coordinates> for RingGeometry {
	/// Creates a new empty ring.
	fn new() -> Self {
		Self(Vec::new())
	}
	/// Returns an immutable reference to the internal list of coordinates.
	fn as_vec(&self) -> &Vec<Coordinates> {
		&self.0
	}
	/// Returns a mutable reference to the internal list of coordinates.
	fn as_mut_vec(&mut self) -> &mut Vec<Coordinates> {
		&mut self.0
	}
	/// Consumes the ring and returns its internal list of coordinates.
	fn into_inner(self) -> Vec<Coordinates> {
		self.0
	}
}

impl Debug for RingGeometry {
	/// Formats the ring for debugging by printing the list of coordinates
	/// in a developer-friendly format.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(RingGeometry, Coordinates);

/// Converts a `geo::LineString<f64>` into a `RingGeometry`, preserving the order of coordinates.
impl From<geo::LineString<f64>> for RingGeometry {
	fn from(geometry: geo::LineString<f64>) -> Self {
		RingGeometry(geometry.into_iter().map(Coordinates::from).collect())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_contains_point_simple_square() {
		// Square from (0,0) to (10,10)
		let ring = RingGeometry::from(&[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]);

		// Inside
		assert!(ring.contains_point(5.0, 5.0));
		assert!(ring.contains_point(1.0, 1.0));
		assert!(ring.contains_point(9.0, 9.0));

		// Outside
		assert!(!ring.contains_point(-1.0, 5.0));
		assert!(!ring.contains_point(11.0, 5.0));
		assert!(!ring.contains_point(5.0, -1.0));
		assert!(!ring.contains_point(5.0, 11.0));
	}

	#[test]
	fn test_contains_point_empty_ring() {
		let ring = RingGeometry::new();
		assert!(!ring.contains_point(0.0, 0.0));
	}

	#[test]
	fn test_to_mercator() {
		// Ring around the equator/prime meridian
		let ring = RingGeometry::from(&[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0]]);
		let mercator = ring.to_mercator();

		// Check that the ring was transformed (coordinates should be in meters now)
		assert!(mercator.0[0].x().abs() > 100_000.0); // ~111km per degree at equator
		assert!(mercator.0[0].y().abs() > 100_000.0);

		// Check that the ring is still closed
		assert_eq!(mercator.0.first(), mercator.0.last());
	}
}
