use anyhow::{ensure, Result};
use std::fmt::Debug;

/// A geographical bounding box, represented by an array of four `f64` values:
/// `[min_x, min_y, max_x, max_y]` or equivalently `[west, south, east, north]`.
///
/// Assumes:
/// - `min_x` (west) is in the range `[-180.0, 180.0]`
/// - `max_x` (east) is in the range `[-180.0, 180.0]`
/// - `min_y` (south) is in the range `[-90.0, 90.0]`
/// - `max_y` (north) is in the range `[-90.0, 90.0]`
///
/// and logically `min_x <= max_x` and `min_y <= max_y`.
#[derive(Clone, Copy, PartialEq)]
pub struct GeoBBox(pub [f64; 4]);

impl GeoBBox {
	/// Attempts to build an optional `GeoBBox` from an optional `Vec<f64>`.
	///
	/// If `input` is `Some`, tries converting that `Vec<f64>` into a `GeoBBox`.
	/// If `input` is `None`, returns `Ok(None)`.
	///
	/// # Errors
	///
	/// Returns an error if the `Vec<f64>` cannot be converted (e.g., wrong length).
	pub fn from_option_vec(input: Option<Vec<f64>>) -> Result<Option<GeoBBox>> {
		match input {
			Some(vec) => Ok(Some(GeoBBox::try_from(vec)?)),
			None => Ok(None),
		}
	}

	/// Returns the bounding box as a `Vec<f64>`.
	///
	/// Typically, this will be four elements: `[west, south, east, north]`.
	pub fn as_vec(&self) -> Vec<f64> {
		self.0.to_vec()
	}

	/// Returns the bounding box as a string in the form:
	/// `"[west,south,east,north]"`
	pub fn as_string(&self) -> String {
		format!(
			"[{}]",
			self
				.0
				.iter()
				.map(|v| v.to_string())
				.collect::<Vec<_>>()
				.join(",")
		)
	}

	/// Expands the current bounding box (in place) so that it includes the area
	/// covered by `other`.
	///
	/// Effectively:
	/// - `min_x` = `min(self.min_x, other.min_x)`
	/// - `min_y` = `min(self.min_y, other.min_y)`
	/// - `max_x` = `max(self.max_x, other.max_x)`
	/// - `max_y` = `max(self.max_y, other.max_y)`
	pub fn extend(&mut self, other: &GeoBBox) {
		let b0 = &mut self.0;
		let b1 = &other.0;
		b0[0] = b0[0].min(b1[0]); // min_x
		b0[1] = b0[1].min(b1[1]); // min_y
		b0[2] = b0[2].max(b1[2]); // max_x
		b0[3] = b0[3].max(b1[3]); // max_y
	}

	/// Returns a new `GeoBBox` that is the result of extending `self` to include
	/// the area covered by `other`.
	///
	/// This is the non-mutating version of [`GeoBBox::extend`].
	pub fn extended(mut self, other: &GeoBBox) -> GeoBBox {
		self.extend(other);
		self
	}

	/// Validates that the bounding box is within the expected lat/lon ranges,
	/// and that the coordinates are in increasing order:
	/// - `min_x >= -180.0`
	/// - `min_y >= -90.0`
	/// - `min_x <= max_x`
	/// - `min_y <= max_y`
	/// - `max_x <= 180.0`
	/// - `max_y <= 90.0`
	///
	/// # Errors
	///
	/// Returns an error if any of these checks fail.
	pub fn check(&self) -> Result<()> {
		ensure!(-180.0 <= self.0[0], "bbox[0] must be >= -180");
		ensure!(-90.0 <= self.0[1], "bbox[1] must be >= -90");
		ensure!(self.0[0] <= self.0[2], "bbox[0] must be <= bbox[2]");
		ensure!(self.0[1] <= self.0[3], "bbox[1] must be <= bbox[3]");
		ensure!(self.0[2] <= 180.0, "bbox[2] must be <= 180");
		ensure!(self.0[3] <= 90.0, "bbox[3] must be <= 90");
		Ok(())
	}
}

impl Debug for GeoBBox {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// Renders the array of four floats directly, e.g., [west, south, east, north]
		write!(f, "{:?}", self.0)
	}
}

impl TryFrom<Vec<f64>> for GeoBBox {
	type Error = anyhow::Error;

	/// Attempts to build a `GeoBBox` from a `Vec<f64>` with exactly four elements
	/// (west, south, east, north).
	///
	/// # Errors
	///
	/// Returns an error if the length is not exactly four.
	fn try_from(input: Vec<f64>) -> Result<Self> {
		ensure!(input.len() == 4, "bbox must have 4 elements");
		let mut bbox = [0.0; 4];
		bbox.copy_from_slice(&input);
		Ok(GeoBBox(bbox))
	}
}
