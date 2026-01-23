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

	fn compute_bounds(&self) -> [f64; 4] {
		let mut x_min = f64::MAX;
		let mut y_min = f64::MAX;
		let mut x_max = f64::MIN;
		let mut y_max = f64::MIN;

		for point in &self.0 {
			let bounds = point.compute_bounds();
			x_min = x_min.min(bounds[0]);
			y_min = y_min.min(bounds[1]);
			x_max = x_max.max(bounds[2]);
			y_max = y_max.max(bounds[3]);
		}

		[x_min, y_min, x_max, y_max]
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
