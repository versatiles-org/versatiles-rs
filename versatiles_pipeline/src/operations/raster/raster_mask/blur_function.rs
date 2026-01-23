//! Blur function enum for edge softening in raster masks.

use std::f64::consts::PI;

/// Defines the interpolation function for blur/edge softening.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlurFunction {
	/// Linear interpolation: alpha = t
	#[default]
	Linear,
	/// Cosine interpolation: alpha = (1 - cos(t * PI)) / 2
	/// Provides smoother transition at edges.
	Cosine,
}

impl BlurFunction {
	/// Interpolate a value in the range [0, 1] using this blur function.
	///
	/// # Arguments
	/// * `t` - Normalized value in [0, 1] range
	///
	/// # Returns
	/// Interpolated value in [0, 1] range
	#[must_use]
	pub fn interpolate(self, t: f64) -> f64 {
		match self {
			BlurFunction::Linear => t,
			BlurFunction::Cosine => (1.0 - (t * PI).cos()) / 2.0,
		}
	}
}

impl TryFrom<&str> for BlurFunction {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		match value.to_lowercase().as_str() {
			"linear" => Ok(BlurFunction::Linear),
			"cosine" => Ok(BlurFunction::Cosine),
			_ => anyhow::bail!("Invalid blur function '{value}'. Expected 'linear' or 'cosine'."),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_linear_interpolation() {
		let f = BlurFunction::Linear;
		assert!((f.interpolate(0.0) - 0.0).abs() < 1e-10);
		assert!((f.interpolate(0.5) - 0.5).abs() < 1e-10);
		assert!((f.interpolate(1.0) - 1.0).abs() < 1e-10);
	}

	#[test]
	fn test_cosine_interpolation() {
		let f = BlurFunction::Cosine;
		assert!((f.interpolate(0.0) - 0.0).abs() < 1e-10);
		assert!((f.interpolate(0.5) - 0.5).abs() < 1e-10);
		assert!((f.interpolate(1.0) - 1.0).abs() < 1e-10);
	}

	#[test]
	fn test_try_from_str() {
		assert_eq!(BlurFunction::try_from("linear").unwrap(), BlurFunction::Linear);
		assert_eq!(BlurFunction::try_from("Linear").unwrap(), BlurFunction::Linear);
		assert_eq!(BlurFunction::try_from("cosine").unwrap(), BlurFunction::Cosine);
		assert_eq!(BlurFunction::try_from("COSINE").unwrap(), BlurFunction::Cosine);
		assert!(BlurFunction::try_from("invalid").is_err());
	}

	#[test]
	fn test_default() {
		assert_eq!(BlurFunction::default(), BlurFunction::Linear);
	}
}
