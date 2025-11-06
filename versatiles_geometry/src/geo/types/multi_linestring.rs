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
