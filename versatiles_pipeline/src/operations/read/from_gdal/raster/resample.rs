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
		use ResampleAlg::*;
		use gdal_sys::GDALResampleAlg::*;
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

/// Default resampling is [`ResampleAlg::Bilinear`].
impl Default for ResampleAlg {
	fn default() -> Self {
		ResampleAlg::Bilinear
	}
}
