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
		let mut p2 = self.0.last().unwrap();
		for p1 in &self.0 {
			sum += (p2.x() - p1.x()) * (p1.y() + p2.y());
			p2 = p1;
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
