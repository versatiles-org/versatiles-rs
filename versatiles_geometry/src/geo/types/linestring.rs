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
