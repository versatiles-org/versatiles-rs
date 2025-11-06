use super::{CompositeGeometryTrait, GeometryTrait, PolygonGeometry};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::JsonValue;

/// Represents a collection of polygons, each of which may have an outer ring and optional inner holes.
/// This struct is used for complex, multi-part areas in 2D space.
#[derive(Clone, PartialEq)]
pub struct MultiPolygonGeometry(pub Vec<PolygonGeometry>);

/// Implementation of `GeometryTrait` for `MultiPolygonGeometry`.
///
/// - `area()` returns the sum of all polygon areas.
/// - `verify()` checks that each polygon is valid.
/// - `to_coord_json()` converts the geometry into a JSON array of polygons,
///   optionally rounding coordinates to a given precision.
impl GeometryTrait for MultiPolygonGeometry {
	fn area(&self) -> f64 {
		self.0.iter().map(super::traits::GeometryTrait::area).sum()
	}

	fn verify(&self) -> Result<()> {
		for line in &self.0 {
			line.verify()?;
		}
		Ok(())
	}

	fn to_coord_json(&self, precision: Option<u8>) -> JsonValue {
		JsonValue::from(
			self
				.0
				.iter()
				.map(|poly| poly.to_coord_json(precision))
				.collect::<Vec<_>>(),
		)
	}
}

/// Implementation of `CompositeGeometryTrait` for `MultiPolygonGeometry`.
///
/// Provides methods for working with the internal list of `PolygonGeometry` objects.
///
/// - `new()` creates an empty `MultiPolygonGeometry`.
/// - `as_vec()` returns an immutable reference to the internal polygons.
/// - `as_mut_vec()` returns a mutable reference to the internal polygons.
/// - `into_inner()` consumes the geometry and returns the vector of polygons.
impl CompositeGeometryTrait<PolygonGeometry> for MultiPolygonGeometry {
	fn new() -> Self {
		Self(Vec::new())
	}
	fn as_vec(&self) -> &Vec<PolygonGeometry> {
		&self.0
	}
	fn as_mut_vec(&mut self) -> &mut Vec<PolygonGeometry> {
		&mut self.0
	}
	fn into_inner(self) -> Vec<PolygonGeometry> {
		self.0
	}
}

/// Implementation of `Debug` for `MultiPolygonGeometry`.
///
/// Prints the list of polygons in a developer-friendly format.
impl Debug for MultiPolygonGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.0).finish()
	}
}

crate::impl_from_array!(MultiPolygonGeometry, PolygonGeometry);
