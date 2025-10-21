use anyhow::{Result, ensure};
use std::fmt::Debug;
use versatiles_derive::context;

static MAX_MERCATOR_LAT: f64 = 85.051_128_779_806_59;
static MAX_MERCATOR_LNG: f64 = 180.0;
static RADIUS: f64 = 6_378_137.0; // meters

/// A geographical bounding box (`GeoBBox`) represents a rectangular area on a map
/// defined by its minimum and maximum longitude (x) and latitude (y) coordinates.
///
/// The bounding box is defined by four `f64` values:
/// - `x_min` (west): Minimum longitude.
/// - `y_min` (south): Minimum latitude.
/// - `x_max` (east): Maximum longitude.
/// - `y_max` (north): Maximum latitude.
///
/// This struct provides methods for creating, manipulating, and validating bounding boxes,
/// as well as converting them to various formats.
///
/// # Examples
///
/// ## Creating a new `GeoBBox`
/// ```
/// use versatiles_core::GeoBBox;
///
/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
/// assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
/// ```
///
/// ## Expanding a bounding box
/// ```
/// use versatiles_core::GeoBBox;
///
/// let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
/// let bbox2 = GeoBBox::new(-12.0, -3.0, 8.0, 6.0).unwrap();
/// bbox1.extend(&bbox2);
/// assert_eq!(bbox1.as_tuple(), (-12.0, -5.0, 10.0, 6.0));
/// ```
#[derive(Clone, Copy, PartialEq)]
#[allow(clippy::manual_non_exhaustive)]
pub struct GeoBBox {
	pub x_min: f64,
	pub y_min: f64,
	pub x_max: f64,
	pub y_max: f64,
	phantom: (),
}

impl GeoBBox {
	/// Creates a new `GeoBBox` from four `f64` values:
	/// `west, south, east, north`.
	///
	/// # Arguments
	/// * `x_min` - Minimum x coordinate (longitude).
	/// * `y_min` - Minimum y coordinate (latitude).
	/// * `x_max` - Maximum x coordinate (longitude).
	/// * `y_max` - Maximum y coordinate (latitude).
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	/// assert_eq!(bbox.x_min, -10.0);
	/// assert_eq!(bbox.y_min, -5.0);
	/// assert_eq!(bbox.x_max, 10.0);
	/// assert_eq!(bbox.y_max, 5.0);
	/// ```
	#[must_use = "GeoBBox::new returns a Result; handle the error or unwrap"]
	pub fn new(x_min: f64, y_min: f64, x_max: f64, y_max: f64) -> Result<GeoBBox> {
		GeoBBox {
			x_min,
			y_min,
			x_max,
			y_max,
			phantom: (),
		}
		.checked()
	}

	pub fn new_save(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<GeoBBox> {
		GeoBBox {
			x_min: x0.min(x1).clamp(-180.0, 180.0),
			y_min: y0.min(y1).clamp(-90.0, 90.0),
			x_max: x0.max(x1).clamp(-180.0, 180.0),
			y_max: y0.max(y1).clamp(-90.0, 90.0),
			phantom: (),
		}
		.checked()
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
	/// use versatiles_core::GeoBBox;
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

	/// Clamps the bounding box *in‑place* to the latitude/longitude limits of the
	/// Web Mercator projection.
	///
	/// Any coordinate outside the valid Mercator span\
	/// (`‒85.05112877980659° ≤ lat ≤ 85.05112877980659°`,\
	/// `‒180° ≤ lon ≤ 180°`) is replaced by the nearest boundary value.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	///
	/// let mut bbox = GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap();
	/// bbox.limit_to_mercator();
	/// assert_eq!(
	///     bbox.as_tuple(),
	///     (-180.0, -85.05112877980659, 180.0, 85.05112877980659)
	/// );
	/// ```
	pub fn limit_to_mercator(&mut self) {
		self.x_min = self.x_min.max(-MAX_MERCATOR_LNG).min(MAX_MERCATOR_LNG); // west
		self.y_min = self.y_min.max(-MAX_MERCATOR_LAT).min(MAX_MERCATOR_LAT); // south
		self.x_max = self.x_max.max(-MAX_MERCATOR_LNG).min(MAX_MERCATOR_LNG); // east
		self.y_max = self.y_max.max(-MAX_MERCATOR_LAT).min(MAX_MERCATOR_LAT); // north
	}

	/// Returns the bounding box as a `Vec<f64>` in the form `[west, south, east, north]`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	/// assert_eq!(bbox.as_vec(), vec![-10.0, -5.0, 10.0, 5.0]);
	/// ```
	#[must_use]
	pub fn as_vec(&self) -> Vec<f64> {
		vec![self.x_min, self.y_min, self.x_max, self.y_max]
	}

	/// Returns the bounding box as a fixed‑size array `[f64; 4]` in the order
	/// `[west, south, east, north]`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	/// assert_eq!(bbox.as_array(), [-10.0, -5.0, 10.0, 5.0]);
	/// ```
	#[must_use]
	pub fn as_array(&self) -> [f64; 4] {
		[self.x_min, self.y_min, self.x_max, self.y_max]
	}

	/// Returns the bounding box as a tuple `(x_min, y_min, x_max, y_max)`.
	#[must_use]
	pub fn as_tuple(&self) -> (f64, f64, f64, f64) {
		(self.x_min, self.y_min, self.x_max, self.y_max)
	}

	/// Returns the bounding box as a string in the form `[x_min, y_min, x_max, y_max]`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	/// assert_eq!(bbox.as_string_json(), "[-10,-5,10,5]");
	/// ```
	#[must_use]
	pub fn as_string_json(&self) -> String {
		format!("[{}]", self.as_string_list())
	}

	/// Returns the bounding box as a string in the form `x_min, y_min, x_max, y_max`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	///
	/// let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	/// assert_eq!(bbox.as_string_list(), "-10,-5,10,5");
	/// ```
	#[must_use]
	pub fn as_string_list(&self) -> String {
		format!("{},{},{},{}", self.x_min, self.y_min, self.x_max, self.y_max)
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
	/// use versatiles_core::GeoBBox;
	///
	/// let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	/// let bbox2 = GeoBBox::new(-12.0, -3.0, 8.0, 6.0).unwrap();
	/// bbox1.extend(&bbox2);
	/// // west = -12, south = -5, east = 10, north = 6
	/// assert_eq!(bbox1.as_tuple(), (-12.0, -5.0, 10.0, 6.0));
	/// ```
	pub fn extend(&mut self, other: &GeoBBox) {
		self.x_min = self.x_min.min(other.x_min); // min_x
		self.y_min = self.y_min.min(other.y_min); // min_y
		self.x_max = self.x_max.max(other.x_max); // max_x
		self.y_max = self.y_max.max(other.y_max); // max_y
	}

	/// Returns a new `GeoBBox` that is the result of extending `self`
	/// to include the area covered by `other`.
	///
	/// This is the non-mutating version of [`extend`](Self::extend).
	#[must_use]
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
	/// use versatiles_core::GeoBBox;
	///
	/// let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	/// let bbox2 = GeoBBox::new(-8.0, -4.0, 12.0, 4.0).unwrap();
	/// bbox1.intersect(&bbox2);
	/// // west = -8, south = -4, east = 10, north = 4
	/// assert_eq!(bbox1.as_tuple(), (-8.0, -4.0, 10.0, 4.0));
	/// ```
	pub fn intersect(&mut self, other: &GeoBBox) {
		self.x_min = self.x_min.max(other.x_min); // min_x
		self.y_min = self.y_min.max(other.y_min); // min_y
		self.x_max = self.x_max.min(other.x_max); // max_x
		self.y_max = self.y_max.min(other.y_max); // max_y
	}

	/// Returns a new `GeoBBox` that is the intersection of `self` and `other`.
	///
	/// This is the non-mutating version of [`intersect`](Self::intersect).
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	///
	/// let bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
	/// let bbox2 = GeoBBox::new(-8.0, -4.0, 12.0, 4.0).unwrap();
	/// let bbox3 = bbox1.intersected(&bbox2);
	/// assert_eq!(bbox3.as_tuple(), (-8.0, -4.0, 10.0, 4.0));
	/// // original remains unchanged
	/// assert_eq!(bbox1.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	/// ```
	#[must_use]
	pub fn intersected(mut self, other: &GeoBBox) -> GeoBBox {
		self.intersect(other);
		self
	}

	fn checked(self) -> Result<Self> {
		ensure!(self.x_min >= -180., "x_min ({}) must be >= -180", self.x_min);
		ensure!(self.y_min >= -90., "y_min ({}) must be >= -90", self.y_min);
		ensure!(self.x_max <= 180., "x_max ({}) must be <= 180", self.x_max);
		ensure!(self.y_max <= 90., "y_max ({}) must be <= 90", self.y_max);
		ensure!(
			self.x_min <= self.x_max,
			"x_min ({}) must be <= x_max ({})",
			self.x_min,
			self.x_max
		);
		ensure!(
			self.y_min <= self.y_max,
			"y_min ({}) must be <= y_max ({})",
			self.y_min,
			self.y_max
		);
		Ok(self)
	}

	/// Convert this WGS84 (EPSG:4326) bounding box to Web‑Mercator meters (EPSG:3857).
	///
	/// Input is interpreted as `[west, south, east, north]` in **degrees** and is
	/// clamped to the valid Web‑Mercator domain
	/// (`-85.05112877980659° ≤ lat ≤ 85.05112877980659°`, `-180° ≤ lon ≤ 180°`).
	pub fn to_mercator(self: &GeoBBox) -> [f64; 4] {
		// Spherical Mercator radius (WGS84 semi-major axis)
		fn x_from_lon(lon_deg: f64) -> f64 {
			let lon = lon_deg.max(-MAX_MERCATOR_LNG).min(MAX_MERCATOR_LNG);
			RADIUS * lon.to_radians()
		}
		fn y_from_lat(lat_deg: f64) -> f64 {
			let lat = lat_deg.max(-MAX_MERCATOR_LAT).min(MAX_MERCATOR_LAT);
			let phi = lat.to_radians();
			RADIUS * ((std::f64::consts::FRAC_PI_4 + phi / 2.0).tan()).ln()
		}

		[
			x_from_lon(self.x_min),
			y_from_lat(self.y_min),
			x_from_lon(self.x_max),
			y_from_lat(self.y_max),
		]
	}
}

impl Debug for GeoBBox {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// Renders the bounding box in the form "GeoBBox(-10, -5, 10, 5)" for example
		write!(
			f,
			"GeoBBox({}, {}, {}, {})",
			self.x_min, self.y_min, self.x_max, self.y_max
		)
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
	/// use versatiles_core::GeoBBox;
	/// use anyhow::Result;
	///
	/// fn example() -> Result<()> {
	///     let input = vec![-10.0, -5.0, 10.0, 5.0];
	///     let bbox = GeoBBox::try_from(input)?;
	///     assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	///     Ok(())
	/// }
	/// ```
	#[context("Failed to convert {input:?} to GeoBBox")]
	fn try_from(input: Vec<f64>) -> Result<Self> {
		ensure!(
			input.len() == 4,
			"GeoBBox must have 4 elements (x_min, y_min, x_max, y_max)"
		);
		GeoBBox::new(input[0], input[1], input[2], input[3])
	}
}

impl TryFrom<[f64; 4]> for GeoBBox {
	type Error = anyhow::Error;
	/// Converts a fixed-size array of four `f64` values into a `GeoBBox`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	/// let bbox = GeoBBox::try_from([-10.0, -5.0, 10.0, 5.0]).unwrap();
	/// assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	/// ```
	fn try_from(input: [f64; 4]) -> Result<Self> {
		GeoBBox::new(input[0], input[1], input[2], input[3])
	}
}

impl<T: Copy + Into<f64>> TryFrom<&[T; 4]> for GeoBBox {
	type Error = anyhow::Error;
	/// Converts a fixed-size array of four numbers into a `GeoBBox`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::GeoBBox;
	/// let bbox = GeoBBox::try_from(&[-10, -5, 10, 5]).unwrap();
	/// assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	/// ```
	fn try_from(input: &[T; 4]) -> Result<Self> {
		GeoBBox::new(input[0].into(), input[1].into(), input[2].into(), input[3].into())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[test]
	fn test_creation() {
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		assert_eq!(bbox.x_min, -10.0);
		assert_eq!(bbox.y_min, -5.0);
		assert_eq!(bbox.x_max, 10.0);
		assert_eq!(bbox.y_max, 5.0);
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
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		assert_eq!(bbox.as_vec(), vec![-10.0, -5.0, 10.0, 5.0]);
		assert_eq!(bbox.as_array(), [-10.0, -5.0, 10.0, 5.0]);
		assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_as_string() {
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		assert_eq!(bbox.as_string_json(), "[-10,-5,10,5]");
		assert_eq!(bbox.as_string_list(), "-10,-5,10,5");
	}

	#[test]
	fn test_extend() {
		let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		let bbox2 = GeoBBox::new(-12.0, -3.0, 8.0, 6.0).unwrap();

		bbox1.extend(&bbox2);
		// Expected west = -12, south = -5, east = 10, north = 6
		assert_eq!(bbox1.as_tuple(), (-12.0, -5.0, 10.0, 6.0));
	}

	#[test]
	fn test_extended() {
		let bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		let bbox2 = GeoBBox::new(-12.0, -3.0, 8.0, 6.0).unwrap();

		let bbox3 = bbox1.extended(&bbox2);
		// Check the new BBox
		assert_eq!(bbox3.as_tuple(), (-12.0, -5.0, 10.0, 6.0));
		// Original remains unchanged
		assert_eq!(bbox1.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_intersect() {
		let mut bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		let bbox2 = GeoBBox::new(-8.0, -4.0, 12.0, 4.0).unwrap();

		bbox1.intersect(&bbox2);
		// Expected west = -8, south = -4, east = 10, north = 4
		assert_eq!(bbox1.as_tuple(), (-8.0, -4.0, 10.0, 4.0));
	}

	#[test]
	fn test_intersected() {
		let bbox1 = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		let bbox2 = GeoBBox::new(-8.0, -4.0, 12.0, 4.0).unwrap();

		let bbox3 = bbox1.intersected(&bbox2);
		// Should produce -8, -4, 10, 4
		assert_eq!(bbox3.as_tuple(), (-8.0, -4.0, 10.0, 4.0));
		// Original remains unchanged
		assert_eq!(bbox1.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_check_valid() {
		// A valid bounding box
		GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap();
	}

	#[test]
	fn test_check_invalid_ranges() {
		// West less than -180
		let bbox = GeoBBox::new(-190.0, -5.0, 10.0, 5.0);
		assert!(bbox.is_err(), "Expected error for west < -180");

		// East greater than 180
		let bbox = GeoBBox::new(-10.0, -5.0, 190.0, 5.0);
		assert!(bbox.is_err(), "Expected error for east > 180");

		// South less than -90
		let bbox = GeoBBox::new(-10.0, -95.0, 10.0, 5.0);
		assert!(bbox.is_err(), "Expected error for south < -90");

		// North greater than 90
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 95.0);
		assert!(bbox.is_err(), "Expected error for north > 90");

		// West > East
		let bbox = GeoBBox::new(10.0, -5.0, -10.0, 5.0);
		assert!(bbox.is_err(), "Expected error for west > east");

		// South > North
		let bbox = GeoBBox::new(-10.0, 6.0, 10.0, 5.0);
		assert!(bbox.is_err(), "Expected error for south > north");
	}

	#[test]
	fn test_limit_to_mercator() {
		let mut bbox = GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap();
		bbox.limit_to_mercator();
		assert_eq!(bbox.as_tuple(), (-180.0, -85.05112877980659, 180.0, 85.05112877980659));
	}

	#[test]
	fn test_try_from_array() {
		let input = [-10.0, -5.0, 10.0, 5.0];
		let bbox = GeoBBox::try_from(input).unwrap();
		assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_try_from_array_ref() {
		let input = &[-10.0, -5.0, 10.0, 5.0];
		let bbox = GeoBBox::try_from(input).unwrap();
		assert_eq!(bbox.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_debug_format() {
		let bbox = GeoBBox::new(-10.0, -5.0, 10.0, 5.0).unwrap();
		assert_eq!(format!("{bbox:?}"), "GeoBBox(-10, -5, 10, 5)");
	}

	#[test]
	fn test_intersect_no_overlap() {
		let mut bbox1 = GeoBBox::new(-10.0, -5.0, 0.0, 0.0).unwrap();
		let bbox2 = GeoBBox::new(1.0, 1.0, 10.0, 5.0).unwrap();
		bbox1.intersect(&bbox2);
		assert_eq!(bbox1.as_tuple(), (1.0, 1.0, 0.0, 0.0)); // No overlap
	}

	#[test]
	fn test_intersected_no_overlap() {
		let bbox1 = GeoBBox::new(-10.0, -5.0, 0.0, 0.0).unwrap();
		let bbox2 = GeoBBox::new(1.0, 1.0, 10.0, 5.0).unwrap();
		let bbox3 = bbox1.intersected(&bbox2);
		assert_eq!(bbox3.as_tuple(), (1.0, 1.0, 0.0, 0.0)); // No overlap
	}

	#[test]
	fn test_extend_with_no_overlap() {
		let mut bbox1 = GeoBBox::new(-10.0, -5.0, 0.0, 0.0).unwrap();
		let bbox2 = GeoBBox::new(1.0, 1.0, 10.0, 5.0).unwrap();
		bbox1.extend(&bbox2);
		assert_eq!(bbox1.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_extended_with_no_overlap() {
		let bbox1 = GeoBBox::new(-10.0, -5.0, 0.0, 0.0).unwrap();
		let bbox2 = GeoBBox::new(1.0, 1.0, 10.0, 5.0).unwrap();
		let bbox3 = bbox1.extended(&bbox2);
		assert_eq!(bbox3.as_tuple(), (-10.0, -5.0, 10.0, 5.0));
	}

	#[test]
	fn test_check_invalid_coordinates() {
		// Invalid: west > east
		let bbox = GeoBBox::new(10.0, -5.0, -10.0, 5.0);
		assert!(bbox.is_err());

		// Invalid: south > north
		let bbox = GeoBBox::new(-10.0, 5.0, 10.0, -5.0);
		assert!(bbox.is_err());
	}

	#[test]
	fn test_check_valid_edge_cases() {
		// Valid: exactly on bounds
		GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap();
	}

	#[test]
	fn test_as_string_edge_cases() {
		let bbox = GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap();
		assert_eq!(bbox.as_string_json(), "[-180,-90,180,90]");
		assert_eq!(bbox.as_string_list(), "-180,-90,180,90");
	}

	#[test]
	fn test_wgs84_as_mercator_world_bounds() {
		let bbox = GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap();
		// Expected Mercator square ~±20037508.342789244
		let [xmin, ymin, xmax, ymax] = bbox.to_mercator();
		let e = 20_037_508.342789244_f64;
		assert!((xmin + e).abs() < 2.0, "xmin={xmin}");
		assert!((ymin + e).abs() < 2.0, "ymin={ymin}");
		assert!((xmax - e).abs() < 2.0, "xmax={xmax}");
		assert!((ymax - e).abs() < 2.0, "ymax={ymax}");
	}

	#[test]
	fn test_wgs84_as_mercator_midlat() {
		let bbox = GeoBBox::new(-10.0, 40.0, 10.0, 50.0).unwrap();
		let [xmin, ymin, xmax, ymax] = bbox.to_mercator();
		assert_eq!(xmin as i32, -1_113_194);
		assert_eq!(xmax as i32, 1_113_194);
		assert_eq!(ymin as i32, 4_865_942);
		assert_eq!(ymax as i32, 6_446_275);
	}

	#[rstest]
	#[case([-180, -90, 180, 90], [-20037508, -20037508, 20037508, 20037508])]
	#[case([-180, -1, 180, 1], [-20037508, -111325, 20037508, 111325])]
	#[case([-1, -90, 1, 90], [-111319, -20037508, 111319, 20037508])]
	fn test_bbox_to_mercator(#[case] input: [i32; 4], #[case] expected: [i32; 4]) {
		let mercator_bbox = GeoBBox::try_from(&input).unwrap().to_mercator();
		assert_eq!(
			[
				mercator_bbox[0] as i32,
				mercator_bbox[1] as i32,
				mercator_bbox[2] as i32,
				mercator_bbox[3] as i32,
			],
			expected
		);
	}

	static MAX_MERCATOR: i64 = 20_037_508_343;

	#[rstest]
	#[case((0.0,0.0),(0,0))]
	#[case((1e-8,1e-8),(1,1))]
	#[case((MAX_MERCATOR_LNG,MAX_MERCATOR_LAT),(MAX_MERCATOR,MAX_MERCATOR))]
	#[case((MAX_MERCATOR_LNG-1e-8,MAX_MERCATOR_LAT-1e-8),(MAX_MERCATOR-1,MAX_MERCATOR-13))]
	fn test_bbox_to_mercator_precision(#[case] point_deg: (f64, f64), #[case] point_mm: (i64, i64)) {
		let mercator_bbox = GeoBBox::try_from(&[-point_deg.0, -point_deg.1, point_deg.0, point_deg.1])
			.unwrap()
			.to_mercator()
			.map(|v| (v * 1_000.0).round() as i64);
		assert_eq!(mercator_bbox, [-point_mm.0, -point_mm.1, point_mm.0, point_mm.1]);
	}
}
