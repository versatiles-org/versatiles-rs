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
pub struct GeoBBox(pub f64, pub f64, pub f64, pub f64);

impl GeoBBox {
	/// Creates a new `GeoBBox` from four `f64` values.
	/// These will be `[x_min, y_min, x_max, y_max]`.
	pub fn new(x_min: f64, y_min: f64, x_max: f64, y_max: f64) -> GeoBBox {
		GeoBBox(x_min, y_min, x_max, y_max)
	}

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

	/// Returns the bounding box as a `Vec<f64>` in the form: `[west, south, east, north]`.
	pub fn as_vec(&self) -> Vec<f64> {
		vec![self.0, self.1, self.2, self.3]
	}

	/// Returns the bounding box as a `[f64;4]` in the form: `[west, south, east, north]`.
	pub fn as_array(&self) -> [f64; 4] {
		[self.0, self.1, self.2, self.3]
	}

	/// Returns the bounding box as a string in the form:
	/// `[west,south,east,north]`
	pub fn as_string_json(&self) -> String {
		format!("[{},{},{},{}]", self.0, self.1, self.2, self.3)
	}

	/// Returns the bounding box as a string in the form:
	/// `west,south,east,north`
	pub fn as_string_list(&self) -> String {
		format!("{},{},{},{}", self.0, self.1, self.2, self.3)
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
		self.0 = self.0.min(other.0); // min_x
		self.1 = self.1.min(other.1); // min_y
		self.2 = self.2.max(other.2); // max_x
		self.3 = self.3.max(other.3); // max_y
	}

	/// Returns a new `GeoBBox` that is the result of extending `self` to include
	/// the area covered by `other`.
	///
	/// This is the non-mutating version of [`GeoBBox::extend`].
	pub fn extended(mut self, other: &GeoBBox) -> GeoBBox {
		self.extend(other);
		self
	}

	/// Expands the current bounding box (in place) so that it includes the area
	/// covered by `other`.
	///
	/// Effectively:
	/// - `min_x` = `min(self.min_x, other.min_x)`
	/// - `min_y` = `min(self.min_y, other.min_y)`
	/// - `max_x` = `max(self.max_x, other.max_x)`
	/// - `max_y` = `max(self.max_y, other.max_y)`
	pub fn intersect(&mut self, other: &GeoBBox) {
		self.0 = self.0.max(other.0); // min_x
		self.1 = self.1.max(other.1); // min_y
		self.2 = self.2.min(other.2); // max_x
		self.3 = self.3.min(other.3); // max_y
	}

	/// Returns a new `GeoBBox` that is the result of extending `self` to include
	/// the area covered by `other`.
	///
	/// This is the non-mutating version of [`GeoBBox::extend`].
	pub fn intersected(mut self, other: &GeoBBox) -> GeoBBox {
		self.intersect(other);
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
		ensure!(self.0 >= -180., "x_min ({}) must be >= -180", self.0);
		ensure!(self.1 >= -90., "y_min ({}) must be >= -90", self.1);
		ensure!(self.2 <= 180., "x_max ({}) must be <= 180", self.2);
		ensure!(self.3 <= 90., "y_max ({}) must be <= 90", self.3);
		ensure!(self.0 <= self.2, "x_min ({}) must be <= x_max ({})", self.0, self.2);
		ensure!(self.1 <= self.3, "y_min ({}) must be <= y_max ({})", self.1, self.3);
		Ok(())
	}
}

impl Debug for GeoBBox {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// Renders the array of four floats directly, e.g., [west, south, east, north]
		f.write_str(&self.as_string_list())
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
		let slice = input.as_slice();
		ensure!(slice.len() == 4, "bbox must have 4 elements");
		Ok(GeoBBox(slice[0], slice[1], slice[2], slice[3]))
	}
}

impl From<&[f64; 4]> for GeoBBox {
	/// Converts a fixed-size array of four `f64` values into a `GeoBBox`.
	fn from(input: &[f64; 4]) -> Self {
		GeoBBox(input[0], input[1], input[2], input[3])
	}
}
