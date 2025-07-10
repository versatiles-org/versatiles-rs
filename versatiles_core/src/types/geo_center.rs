use anyhow::{Result, ensure};
use std::fmt::Debug;

/// A center point in geographic space, represented by:
/// - `f64` longitude, in the range `[-180, 180]`
/// - `f64` latitude, in the range `[-90, 90]`
/// - `u8` zoom level, typically in the range `[0, 30]`
#[derive(Clone, Copy, PartialEq)]
pub struct GeoCenter(pub f64, pub f64, pub u8);

impl GeoCenter {
	/// Tries to construct an optional `GeoCenter` from an `Option<Vec<f64>>`.
	///
	/// - If the input is `Some(vec)`, attempts a conversion via [`GeoCenter::try_from`].
	/// - If the input is `None`, returns `Ok(None)`.
	///
	/// # Errors
	///
	/// Returns an error if the vector has the wrong length or contains invalid data
	/// (see [`TryFrom<Vec<f64>> for GeoCenter`]).
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoCenter;
	/// use anyhow::Result;
	///
	/// fn example() -> Result<()> {
	///     let data = Some(vec![100.0, 45.0, 5.0]);
	///     let center_opt = GeoCenter::from_option_vec(data)?;
	///     assert!(center_opt.is_some());
	///     
	///     let none_data: Option<Vec<f64>> = None;
	///     let none_center = GeoCenter::from_option_vec(none_data)?;
	///     assert!(none_center.is_none());
	///     
	///     Ok(())
	/// }
	/// ```
	pub fn from_option_vec(input: Option<Vec<f64>>) -> Result<Option<Self>> {
		match input {
			Some(vec) => Ok(Some(GeoCenter::try_from(vec)?)),
			None => Ok(None),
		}
	}

	/// Converts the `GeoCenter` into a `Vec<f64>` in the form `[longitude, latitude, zoom]`.
	///
	/// Note that `zoom` is cast to `f64`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoCenter;
	///
	/// let gc = GeoCenter(12.3, 45.6, 7);
	/// let vec = gc.as_vec();
	/// assert_eq!(vec, vec![12.3, 45.6, 7.0]);
	/// ```
	pub fn as_vec(&self) -> Vec<f64> {
		vec![self.0, self.1, self.2 as f64]
	}

	/// Converts the `GeoCenter` into a fixed-size array `[f64; 3]` in the form
	/// `[longitude, latitude, zoom]`.
	///
	/// Note that `zoom` is cast to `f64`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoCenter;
	///
	/// let gc = GeoCenter(-75.5, 40.2, 3);
	/// let arr = gc.as_array();
	/// assert_eq!(arr, [-75.5, 40.2, 3.0]);
	/// ```
	pub fn as_array(&self) -> [f64; 3] {
		[self.0, self.1, self.2 as f64]
	}

	/// Checks that the stored longitude, latitude, and zoom are within valid ranges:
	/// - longitude in `[-180, 180]`
	/// - latitude in `[-90, 90]`
	/// - zoom up to `30`
	///
	/// # Errors
	///
	/// Returns an error if any checks fail.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoCenter;
	/// use anyhow::Result;
	///
	/// fn validate_center() -> Result<()> {
	///     let center = GeoCenter(100.0, -45.0, 8);
	///     center.check()?;  // should succeed if within valid ranges
	///     Ok(())
	/// }
	/// ```
	pub fn check(&self) -> Result<()> {
		ensure!(-180.0 <= self.0, "center[0] (longitude) must be >= -180");
		ensure!(-90.0 <= self.1, "center[1] (latitude) must be >= -90");
		ensure!(self.0 <= 180.0, "center[0] (longitude) must be <= 180");
		ensure!(self.1 <= 90.0, "center[1] (latitude) must be <= 90");
		ensure!(self.2 <= 30, "center[2] (zoom) must be <= 30");
		Ok(())
	}
}

impl Debug for GeoCenter {
	/// Formats the `GeoCenter` as `"longitude, latitude (zoom)"`.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}, {} ({})", self.0, self.1, self.2)
	}
}

impl TryFrom<Vec<f64>> for GeoCenter {
	type Error = anyhow::Error;

	/// Attempts to construct a `GeoCenter` from a `Vec<f64>` with exactly three elements:
	/// `[longitude, latitude, zoom]`.
	///
	/// # Errors
	///
	/// Returns an error if the length is not exactly three.
	/// If desired, you could also validate numeric bounds here before casting
	/// `zoom` to `u8`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::types::GeoCenter;
	/// use anyhow::Result;
	///
	/// fn example() -> Result<()> {
	///     let vec = vec![-75.5, 40.2, 4.0];
	///     let center = GeoCenter::try_from(vec)?;
	///     assert_eq!(center, GeoCenter(-75.5, 40.2, 4));
	///     Ok(())
	/// }
	/// ```
	fn try_from(input: Vec<f64>) -> Result<Self> {
		ensure!(
			input.len() == 3,
			"center must have 3 elements: [longitude, latitude, zoom]"
		);
		Ok(GeoCenter(input[0], input[1], input[2] as u8))
	}
}

#[cfg(test)]
mod tests {
	use super::GeoCenter;
	use anyhow::Result;
	use std::convert::TryFrom;

	#[test]
	fn test_from_option_vec() -> Result<()> {
		// Some valid data
		let data = Some(vec![123.4, -56.7, 8.0]);
		let gc_opt = GeoCenter::from_option_vec(data)?;
		assert!(gc_opt.is_some());
		let gc = gc_opt.unwrap();
		assert_eq!(gc, GeoCenter(123.4, -56.7, 8));

		// None data
		let none_data: Option<Vec<f64>> = None;
		let gc_none = GeoCenter::from_option_vec(none_data)?;
		assert!(gc_none.is_none());

		Ok(())
	}

	#[test]
	fn test_try_from_valid() -> Result<()> {
		let vec = vec![0.0, 0.0, 10.0];
		let gc = GeoCenter::try_from(vec)?;
		assert_eq!(gc, GeoCenter(0.0, 0.0, 10));
		Ok(())
	}

	#[test]
	fn test_try_from_invalid_len() {
		let vec = vec![1.0, 2.0];
		let result = GeoCenter::try_from(vec);
		assert!(result.is_err(), "Expected error for < 3 elements");
	}

	#[test]
	fn test_as_vec_and_array() {
		let gc = GeoCenter(-75.0, 40.0, 5);
		assert_eq!(gc.as_vec(), vec![-75.0, 40.0, 5.0]);
		assert_eq!(gc.as_array(), [-75.0, 40.0, 5.0]);
	}

	#[test]
	fn test_check_valid() -> Result<()> {
		let gc = GeoCenter(180.0, 90.0, 30);
		gc.check()?; // Should pass
		Ok(())
	}

	#[test]
	fn test_check_invalid_longitude() {
		let gc_low = GeoCenter(-200.0, 0.0, 10);
		assert!(gc_low.check().is_err(), "Expected error for longitude < -180");

		let gc_high = GeoCenter(200.0, 0.0, 10);
		assert!(gc_high.check().is_err(), "Expected error for longitude > 180");
	}

	#[test]
	fn test_check_invalid_latitude() {
		let gc_low = GeoCenter(0.0, -95.0, 10);
		assert!(gc_low.check().is_err(), "Expected error for latitude < -90");

		let gc_high = GeoCenter(0.0, 95.0, 10);
		assert!(gc_high.check().is_err(), "Expected error for latitude > 90");
	}

	#[test]
	fn test_check_invalid_zoom() {
		let gc_zoom = GeoCenter(0.0, 0.0, 31);
		assert!(gc_zoom.check().is_err(), "Expected error for zoom > 30");
	}

	#[test]
	fn test_debug_format() {
		let gc = GeoCenter(12.3456, -7.89, 9);
		let debug_str = format!("{gc:?}");
		assert_eq!(debug_str, "12.3456, -7.89 (9)");
	}
}
