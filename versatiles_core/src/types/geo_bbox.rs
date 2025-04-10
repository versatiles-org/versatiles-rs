use anyhow::{ensure, Result};
use std::fmt::Debug;

/// A geographic bounding box, represented by four `f64` values:
/// `[west, south, east, north]` (sometimes referred to as `[min_x, min_y, max_x, max_y]`).
///
/// Assumes:
/// - `min_x` (west) is in `[-180.0, 180.0]`
/// - `min_y` (south) is in `[-90.0, 90.0]`
/// - `max_x` (east) is in `[-180.0, 180.0]`
/// - `max_y` (north) is in `[-90.0, 90.0]`
/// - `west <= east`
/// - `south <= north`
///
/// These constraints can be verified using the [`check`](GeoBBox::check) method.
#[derive(Clone, Copy, PartialEq)]
pub struct GeoBBox(pub f64, pub f64, pub f64, pub f64);

impl GeoBBox {
	/// Creates a new `GeoBBox` from four `f64` values:
	/// `west, south, east, north`.
	///
	/// # Arguments
	/// * `x_min`  - Minimum x coordinate (longitude).
	/// * `y_min` - Minimum y coordinate (latitude).
	/// * `x_max`  - Maximum x coordinate (longitude).
	/// * `y_max` - Maximum y coordinate (latitude).
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
	/// assert_eq!(bbox.0, -10.0);
	/// assert_eq!(bbox.1, -5.0);
	/// assert_eq!(bbox.2, 10.0);
	/// assert_eq!(bbox.3, 5.0);
	/// ```
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
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	/// use anyhow::Result;
	///
	/// fn example() -> Result<()> {
	///     let some_vec = Some(vec![-10.0, -5.0, 10.0, 5.0]);
	///     let maybe_bbox = GeoBBox::from_option_vec(some_vec)?;
	///     assert!(maybe_bbox.is_some());
	///
	///     let none_vec: Option<Vec<f64>> = None;
	///     let maybe_bbox2 = GeoBBox::from_option_vec(none_vec)?;
	///     assert!(maybe_bbox2.is_none());
	///     Ok(())
	/// }
	/// ```
	pub fn from_option_vec(input: Option<Vec<f64>>) -> Result<Option<GeoBBox>> {
		match input {
			Some(vec) => Ok(Some(GeoBBox::try_from(vec)?)),
			None => Ok(None),
		}
	}

	/// Returns the bounding box as a `Vec<f64>` in the form `[west, south, east, north]`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
	/// assert_eq!(bbox.as_vec(), vec![-10.0, -5.0, 10.0, 5.0]);
	/// ```
	pub fn as_vec(&self) -> Vec<f64> {
		vec![self.0, self.1, self.2, self.3]
	}

	/// Returns the bounding box as an array `[f64; 4]` in the form `[west, south, east, north]`.
	pub fn as_array(&self) -> [f64; 4] {
		[self.0, self.1, self.2, self.3]
	}

	/// Returns the bounding box as a tuple `(x_min, y_min, x_max, y_max)`.
	pub fn as_tuple(&self) -> (f64, f64, f64, f64) {
		(self.0, self.1, self.2, self.3)
	}

	/// Returns the bounding box as a string in the form `[x_min, y_min, x_max, y_max]`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
	/// assert_eq!(bbox.as_string_json(), "[-10,-5,10,5]");
	/// ```
	pub fn as_string_json(&self) -> String {
		format!("[{},{},{},{}]", self.0, self.1, self.2, self.3)
	}

	/// Returns the bounding box as a string in the form `x_min, y_min, x_max, y_max`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
	/// assert_eq!(bbox.as_string_list(), "-10,-5,10,5");
	/// ```
	pub fn as_string_list(&self) -> String {
		format!("{},{},{},{}", self.0, self.1, self.2, self.3)
	}

	/// Expands the current bounding box in place so that it includes the area
	/// covered by `other`.
	///
	/// This is equivalent to:
	/// - `min_x` = `min(self.min_x, other.min_x)`
	/// - `min_y` = `min(self.min_y, other.min_y)`
	/// - `max_x` = `max(self.max_x, other.max_x)`
	/// - `max_y` = `max(self.max_y, other.max_y)`
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	///
	/// let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
	/// let bbox2 = GeoBBox::new(-12.0, -3.0, 8.0, 6.0);
	/// bbox1.extend(&bbox2);
	/// // west = -12, south = -5, east = 10, north = 6
	/// assert_eq!(bbox1.as_tuple(), (-12.0, -5.0, 10.0, 6.0));
	/// ```
	pub fn extend(&mut self, other: &GeoBBox) {
		self.0 = self.0.min(other.0); // min_x
		self.1 = self.1.min(other.1); // min_y
		self.2 = self.2.max(other.2); // max_x
		self.3 = self.3.max(other.3); // max_y
	}

	/// Returns a new `GeoBBox` that is the result of extending `self`
	/// to include the area covered by `other`.
	///
	/// This is the non-mutating version of [`extend`](Self::extend).
	pub fn extended(mut self, other: &GeoBBox) -> GeoBBox {
		self.extend(other);
		self
	}

	/// Intersects the current bounding box in place so that it includes *only*
	/// the overlapping area covered by both `self` and `other`.
	///
	/// This is equivalent to:
	/// - `min_x` = `min(self.min_x, other.min_x)`
	/// - `min_y` = `min(self.min_y, other.min_y)`
	/// - `max_x` = `max(self.max_x, other.max_x)`
	/// - `max_y` = `max(self.max_y, other.max_y)`
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	///
	/// let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
	/// let bbox2 = GeoBBox::new(-8.0, -4.0, 12.0, 4.0);
	/// bbox1.intersect(&bbox2);
	/// // west = -8, south = -4, east = 10, north = 4
	/// assert_eq!(bbox1.as_tuple(), (-8.0, -4.0, 10.0, 4.0));
	/// ```
	pub fn intersect(&mut self, other: &GeoBBox) {
		self.0 = self.0.max(other.0); // min_x
		self.1 = self.1.max(other.1); // min_y
		self.2 = self.2.min(other.2); // max_x
		self.3 = self.3.min(other.3); // max_y
	}

	/// Returns a new `GeoBBox` that is the intersection of `self` and `other`.
	///
	/// This is the non-mutating version of [`intersect`](Self::intersect).
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	///
	/// let bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
	/// let bbox2 = GeoBBox::new(-8.0, -4.0, 12.0, 4.0);
	/// let bbox3 = bbox1.intersected(&bbox2);
	/// assert_eq!(bbox3.as_tuple(), (-8.0, -4.0, 10.0, 4.0));
	/// // original remains unchanged
	/// assert_eq!(bbox1.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	/// ```
	pub fn intersected(mut self, other: &GeoBBox) -> GeoBBox {
		self.intersect(other);
		self
	}

	/// Validates that the bounding box is within the typical lat/lon ranges,
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
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	/// use anyhow::Result;
	///
	/// fn validate_bbox() -> Result<()> {
	///     let bbox = GeoBBox::new(-180.0, -90.0, 180.0, 90.0);
	///     bbox.check()?;
	///     Ok(())
	/// }
	/// ```
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
		// Renders the bounding box in the form "GeoBBox(-10, -5, 10, 5)" for example
		write!(f, "GeoBBox({}, {}, {}, {})", self.0, self.1, self.2, self.3)
	}
}

impl TryFrom<Vec<f64>> for GeoBBox {
	type Error = anyhow::Error;

	/// Attempts to build a `GeoBBox` from a `Vec<f64>` with exactly four elements
	/// `[west, south, east, north]`.
	///
	/// # Errors
	///
	/// Returns an error if the length is not exactly four.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	/// use anyhow::Result;
	///
	/// fn example() -> Result<()> {
	///     let input = vec![-10.0, -5.0, 10.0, 5.0];
	///     let bbox = GeoBBox::try_from(input)?;
	///     assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	///     Ok(())
	/// }
	/// ```
	fn try_from(input: Vec<f64>) -> Result<Self> {
		ensure!(
			input.len() == 4,
			"GeoBBox must have 4 elements (x_min, y_min, x_max, y_max)"
		);
		Ok(GeoBBox(input[0], input[1], input[2], input[3]))
	}
}

impl From<&[f64; 4]> for GeoBBox {
	/// Converts a fixed-size array of four `f64` values into a `GeoBBox`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoBBox;
	///
	/// let arr = [-10.0, -5.0, 10.0, 5.0];
	/// let bbox = GeoBBox::from(&arr);
	/// assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	/// ```
	fn from(input: &[f64; 4]) -> Self {
		GeoBBox(input[0], input[1], input[2], input[3])
	}
}

#[cfg(test)]
mod tests {
	use super::GeoBBox;
	use anyhow::Result;

	#[test]
	fn test_creation() {
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
		assert_eq!(bbox.0, -10.0);
		assert_eq!(bbox.1, -5.0);
		assert_eq!(bbox.2, 10.0);
		assert_eq!(bbox.3, 5.0);
	}

	#[test]
	fn test_from_option_vec() -> Result<()> {
		// Some valid Vec
		let input = Some(vec![1.0, 2.0, 3.0, 4.0]);
		let maybe_bbox = GeoBBox::from_option_vec(input)?;
		assert!(maybe_bbox.is_some());
		let bbox = maybe_bbox.unwrap();
		assert_eq!(bbox.as_tuple(), (1.0, 2.0, 3.0, 4.0));

		// None
		let input_none: Option<Vec<f64>> = None;
		let maybe_bbox_none = GeoBBox::from_option_vec(input_none)?;
		assert!(maybe_bbox_none.is_none());
		Ok(())
	}

	#[test]
	fn test_try_from_vec_valid() -> Result<()> {
		let input = vec![-10.0, -5.0, 10.0, 5.0];
		let bbox = GeoBBox::try_from(input)?;
		assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
		Ok(())
	}

	#[test]
	fn test_try_from_vec_invalid_length() {
		let input = vec![-10.0, -5.0, 10.0];
		let result = GeoBBox::try_from(input);
		assert!(result.is_err(), "Expected error for length != 4");
	}

	#[test]
	fn test_as_vec_array_tuple() {
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
		assert_eq!(bbox.as_vec(), vec![-10.0, -5.0, 10.0, 5.0]);
		assert_eq!(bbox.as_array(), [-10.0, -5.0, 10.0, 5.0]);
		assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_as_string() {
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
		assert_eq!(bbox.as_string_json(), "[-10,-5,10,5]");
		assert_eq!(bbox.as_string_list(), "-10,-5,10,5");
	}

	#[test]
	fn test_extend() {
		let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
		let bbox2 = GeoBBox::new(-12.0, -3.0, 8.0, 6.0);

		bbox1.extend(&bbox2);
		// Expected west = -12, south = -5, east = 10, north = 6
		assert_eq!(bbox1.as_tuple(), (-12.0, -5.0, 10.0, 6.0));
	}

	#[test]
	fn test_extended() {
		let bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
		let bbox2 = GeoBBox::new(-12.0, -3.0, 8.0, 6.0);

		let bbox3 = bbox1.extended(&bbox2);
		// Check the new BBox
		assert_eq!(bbox3.as_tuple(), (-12.0, -5.0, 10.0, 6.0));
		// Original remains unchanged
		assert_eq!(bbox1.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_intersect() {
		let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
		let bbox2 = GeoBBox::new(-8.0, -4.0, 12.0, 4.0);

		bbox1.intersect(&bbox2);
		// Expected west = -8, south = -4, east = 10, north = 4
		assert_eq!(bbox1.as_tuple(), (-8.0, -4.0, 10.0, 4.0));
	}

	#[test]
	fn test_intersected() {
		let bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0);
		let bbox2 = GeoBBox::new(-8.0, -4.0, 12.0, 4.0);

		let bbox3 = bbox1.intersected(&bbox2);
		// Should produce -8, -4, 10, 4
		assert_eq!(bbox3.as_tuple(), (-8.0, -4.0, 10.0, 4.0));
		// Original remains unchanged
		assert_eq!(bbox1.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_check_valid() -> Result<()> {
		// A valid bounding box
		let bbox = GeoBBox::new(-180.0, -90.0, 180.0, 90.0);
		bbox.check()?; // should succeed
		Ok(())
	}

	#[test]
	fn test_check_invalid_ranges() {
		// West less than -180
		let bbox = GeoBBox::new(-190.0, -5.0, 10.0, 5.0);
		assert!(bbox.check().is_err(), "Expected error for west < -180");

		// East greater than 180
		let bbox = GeoBBox::new(-10.0, -5.0, 190.0, 5.0);
		assert!(bbox.check().is_err(), "Expected error for east > 180");

		// South less than -90
		let bbox = GeoBBox::new(-10.0, -95.0, 10.0, 5.0);
		assert!(bbox.check().is_err(), "Expected error for south < -90");

		// North greater than 90
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 95.0);
		assert!(bbox.check().is_err(), "Expected error for north > 90");

		// West > East
		let bbox = GeoBBox::new(10.0, -5.0, -10.0, 5.0);
		assert!(bbox.check().is_err(), "Expected error for west > east");

		// South > North
		let bbox = GeoBBox::new(-10.0, 6.0, 10.0, 5.0);
		assert!(bbox.check().is_err(), "Expected error for south > north");
	}
}
