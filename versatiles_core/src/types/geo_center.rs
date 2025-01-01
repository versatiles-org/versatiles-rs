use anyhow::{ensure, Result};
use std::fmt::Debug;

/// A center point in geographic space, represented by:
/// - `f64` longitude (range: [-180, 180])
/// - `f64` latitude (range: [-90, 90])
/// - `u8` zoom level (typical range: 0 to 30)
#[derive(Clone, Copy, PartialEq)]
pub struct GeoCenter(pub f64, pub f64, pub u8);

impl GeoCenter {
	/// Tries to construct an optional `GeoCenter` from an optional `Vec<f64>`.
	///
	/// If the input is `Some`, attempts a conversion via `GeoCenter::try_from`.
	/// If the input is `None`, returns `Ok(None)`.
	///
	/// # Errors
	///
	/// Returns an error if the vector has the wrong length or invalid data.
	pub fn from_option_vec(input: Option<Vec<f64>>) -> Result<Option<Self>> {
		match input {
			Some(vec) => Ok(Some(GeoCenter::try_from(vec)?)),
			None => Ok(None),
		}
	}

	/// Converts the `GeoCenter` into a `Vec<f64>` in the form:
	/// `[longitude, latitude, zoom]`.
	///
	/// Note that `zoom` is cast to `f64`.
	pub fn as_vec(&self) -> Vec<f64> {
		vec![self.0, self.1, self.2 as f64]
	}

	/// Checks that the stored longitude, latitude, and zoom are within valid ranges:
	/// - longitude between -180 and 180
	/// - latitude between -90 and 90
	/// - zoom up to 30
	///
	/// # Errors
	///
	/// Returns an error if any of these checks fail.
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
	fn try_from(input: Vec<f64>) -> Result<Self> {
		ensure!(
			input.len() == 3,
			"center must have 3 elements: [longitude, latitude, zoom]"
		);
		Ok(GeoCenter(input[0], input[1], input[2] as u8))
	}
}
