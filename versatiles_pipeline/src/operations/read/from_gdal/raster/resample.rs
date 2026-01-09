//!
//! Resampling algorithm mapping for GDAL operations.
//!
//! This module defines [`ResampleAlg`], a simple enum that mirrors GDAL’s
//! [`GDALResampleAlg`] constants. It is used to select how raster values are
//! interpolated or aggregated when reprojecting, rescaling, or warping datasets.
//!
//! # Overview
//! - `NearestNeighbour`: Picks the closest pixel value (fastest, blocky).
//! - `Bilinear`: Interpolates using a 2×2 neighborhood (smooth, default).
//! - `Cubic`: 4×4 kernel cubic convolution approximation.
//! - `CubicSpline`: 4×4 kernel cubic B‑spline approximation (smoother).
//! - `Lanczos`: 6×6 kernel Lanczos windowed sinc (highest quality).
//! - `Average`: Weighted average of all non‑NoData pixels intersecting the output.
//!
//! See [GDALWarpResample](https://gdal.org/api/gdalwarp_cpp.html) for details.

#[allow(dead_code)]
/// Enumeration of resampling algorithms compatible with GDAL.
///
/// These values are mapped 1‑to‑1 to GDAL’s [`GDALResampleAlg`] constants via [`to_gdal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResampleAlg {
	/// Nearest neighbour — fastest, no smoothing.
	NearestNeighbour,
	/// Bilinear interpolation using a 2×2 pixel kernel.
	Bilinear,
	/// Cubic convolution approximation using a 4×4 kernel.
	Cubic,
	/// Cubic B‑Spline interpolation using a 4×4 kernel.
	CubicSpline,
	/// Lanczos windowed sinc interpolation (6×6 kernel).
	Lanczos,
	/// Weighted average of all non‑NoData contributing pixels.
	Average,
}

impl ResampleAlg {
	/// Convert this [`ResampleAlg`] to its corresponding GDAL constant (`GDALResampleAlg`).
	pub fn as_gdal(&self) -> u32 {
		use ResampleAlg::{Average, Bilinear, Cubic, CubicSpline, Lanczos, NearestNeighbour};
		use gdal_sys::GDALResampleAlg::{
			GRA_Average, GRA_Bilinear, GRA_Cubic, GRA_CubicSpline, GRA_Lanczos, GRA_NearestNeighbour,
		};
		match self {
			NearestNeighbour => GRA_NearestNeighbour,
			Bilinear => GRA_Bilinear,
			Cubic => GRA_Cubic,
			CubicSpline => GRA_CubicSpline,
			Lanczos => GRA_Lanczos,
			Average => GRA_Average,
		}
	}
}

/// Default resampling is [`ResampleAlg::Average`].
impl Default for ResampleAlg {
	fn default() -> Self {
		ResampleAlg::Average
	}
}

#[cfg(test)]
mod tests {
	use super::ResampleAlg;

	#[test]
	fn maps_to_gdal_constants() {
		use gdal_sys::GDALResampleAlg::*;
		assert_eq!(ResampleAlg::NearestNeighbour.as_gdal(), GRA_NearestNeighbour);
		assert_eq!(ResampleAlg::Bilinear.as_gdal(), GRA_Bilinear);
		assert_eq!(ResampleAlg::Cubic.as_gdal(), GRA_Cubic);
		assert_eq!(ResampleAlg::CubicSpline.as_gdal(), GRA_CubicSpline);
		assert_eq!(ResampleAlg::Lanczos.as_gdal(), GRA_Lanczos);
		assert_eq!(ResampleAlg::Average.as_gdal(), GRA_Average);
	}

	#[test]
	fn default_is_average() {
		assert!(matches!(ResampleAlg::default(), ResampleAlg::Average));
	}

	#[test]
	fn mapping_values_are_unique() {
		use std::collections::HashSet;
		let vals: HashSet<u32> = [
			ResampleAlg::NearestNeighbour.as_gdal(),
			ResampleAlg::Bilinear.as_gdal(),
			ResampleAlg::Cubic.as_gdal(),
			ResampleAlg::CubicSpline.as_gdal(),
			ResampleAlg::Lanczos.as_gdal(),
			ResampleAlg::Average.as_gdal(),
		]
		.into_iter()
		.collect();
		assert_eq!(vals.len(), 6, "duplicate GDALResampleAlg values detected");
	}
}
