//! Image metadata and comparison utilities for `DynamicImage`.
//!
//! This module defines [`DynamicImageTraitInfo`], which augments `image::DynamicImage` with
//! lightweight, allocation-free helpers for:
//!
//! - Introspecting pixel layout: bits per value and channel count
//! - Validating compatibility between images (same size / same color model)
//! - Computing simple per-channel differences between two images
//! - Determining transparency characteristics (empty/opaque) and mapping empty images to `None`
//!
//! The trait builds on top of [`super::convert::DynamicImageTraitConvert`], notably its
//! `iter_pixels()` method for zero-copy pixel traversal.
use super::convert::DynamicImageTraitConvert;
use anyhow::{Result, ensure};
use image::{DynamicImage, ExtendedColorType};

/// Utilities to inspect/compare images and reason about alpha while avoiding extra allocations.
pub trait DynamicImageTraitInfo: DynamicImageTraitConvert {
	/// Returns the number of **bits per single channel value** (e.g. `8` for `Rgb8`, `La8`).
	fn bits_per_value(&self) -> u8;

	/// Returns the number of **channels** in the image (1, 2, 3 or 4 for 8‑bit variants).
	fn channel_count(&self) -> u8;

	/// Computes a **per-channel difference score** against `other`.
	///
	/// The score for channel *i* is `ceil(10 * SSE_i / N) / 10`, where `SSE_i` is the sum of
	/// squared per-pixel differences for that channel and `N = width * height`.
	/// This yields a value rounded up to one decimal place.
	///
	/// Errors if the images differ in size or color model.
	fn diff(&self, other: &DynamicImage) -> Result<Vec<f64>>;

	/// Ensures both images share the **same size and color model**.
	///
	/// Returns `Ok(())` on success; otherwise returns an error describing the mismatch.
	fn ensure_same_meta(&self, other: &DynamicImage) -> Result<()>;

	/// Ensures both images share the **same dimensions** (`width` and `height`).
	///
	/// Returns `Ok(())` on success; otherwise returns an error describing the mismatch.
	fn ensure_same_size(&self, other: &DynamicImage) -> Result<()>;

	/// Returns the image's [`ExtendedColorType`], e.g. `L8`, `La8`, `Rgb8`, or `Rgba8`.
	fn extended_color_type(&self) -> ExtendedColorType;

	/// Converts the image into `Option<DynamicImage>`: returns `None` when the image is considered
	/// **empty** (has an alpha channel and all alpha values are `0`), otherwise returns `Some(self)`.
	fn into_optional(self) -> Option<DynamicImage>;

	/// Returns `true` when the image has an alpha channel and **all alpha values are `0`**.
	/// Images **without** an alpha channel are never considered empty.
	fn is_empty(&self) -> bool;

	/// Returns `true` when the image has an alpha channel and **all alpha values are `255`**.
	/// Images **without** an alpha channel are treated as fully opaque (`true`).
	fn is_opaque(&self) -> bool;
}

impl DynamicImageTraitInfo for DynamicImage
where
	DynamicImage: DynamicImageTraitConvert,
{
	fn bits_per_value(&self) -> u8 {
		(self.color().bits_per_pixel() / u16::from(self.color().channel_count())) as u8
	}

	fn channel_count(&self) -> u8 {
		self.color().channel_count()
	}

	fn diff(&self, other: &DynamicImage) -> Result<Vec<f64>> {
		self.ensure_same_meta(other)?;

		let channels = self.color().channel_count() as usize;
		let mut sqr_sum = vec![0u64; channels];

		for (p1, p2) in self.iter_pixels().zip(other.iter_pixels()) {
			for i in 0..channels {
				let d = i64::from(p1[i]) - i64::from(p2[i]);
				sqr_sum[i] += (d * d) as u64;
			}
		}

		let n = f64::from(self.width() * self.height());
		Ok(sqr_sum.iter().map(|v| (10.0 * (*v as f64) / n).ceil() / 10.0).collect())
	}

	fn ensure_same_meta(&self, other: &DynamicImage) -> Result<()> {
		self.ensure_same_size(other)?;
		ensure!(
			self.color() == other.color(),
			"Pixel value type mismatch: self has {:?}, but the other image has {:?}",
			self.color(),
			other.color()
		);
		Ok(())
	}

	fn ensure_same_size(&self, other: &DynamicImage) -> Result<()> {
		ensure!(
			self.width() == other.width(),
			"Image width mismatch: self has width {}, but the other image has width {}",
			self.width(),
			other.width()
		);
		ensure!(
			self.height() == other.height(),
			"Image height mismatch: self has height {}, but the other image has height {}",
			self.height(),
			other.height()
		);
		Ok(())
	}

	fn extended_color_type(&self) -> ExtendedColorType {
		self.color().into()
	}

	fn into_optional(self) -> Option<DynamicImage> {
		if self.is_empty() { None } else { Some(self) }
	}

	fn is_empty(&self) -> bool {
		if !self.color().has_alpha() {
			return false;
		}
		let alpha_channel = (self.color().channel_count() - 1) as usize;
		return self.iter_pixels().all(|p| p[alpha_channel] == 0);
	}

	fn is_opaque(&self) -> bool {
		if !self.color().has_alpha() {
			return true;
		}
		let alpha_channel = (self.color().channel_count() - 1) as usize;
		return self.iter_pixels().all(|p| p[alpha_channel] == 255);
	}
}

/// Tests cover metadata queries, size/meta validation, empty/opaque logic and per-channel diffs.
#[cfg(test)]
mod tests {
	use super::*;
	use image::ExtendedColorType;
	use rstest::rstest;

	// --- helpers -----------------------------------------------------------
	fn sample_l8() -> DynamicImage {
		DynamicImage::from_fn_l8(4, 3, |x, y| ((x + y) % 2) as u8)
	}
	fn sample_la8(alpha: u8) -> DynamicImage {
		DynamicImage::from_fn_la8(4, 3, |x, y| [((x * 2 + y) % 256) as u8, alpha])
	}
	fn sample_rgb8() -> DynamicImage {
		DynamicImage::from_fn_rgb8(4, 3, |x, y| [x as u8, y as u8, (x + y) as u8])
	}
	fn sample_rgba8(alpha: u8) -> DynamicImage {
		DynamicImage::from_fn_rgba8(4, 3, |x, y| [x as u8, y as u8, (x + y) as u8, alpha])
	}

	// --- bits_per_value & channel_count -----------------------------------
	#[rstest]
	#[case::l8(sample_l8(), 8u8, 1u8)]
	#[case::la8(sample_la8(255), 8u8, 2u8)]
	#[case::rgb8(sample_rgb8(), 8u8, 3u8)]
	#[case::rgba8(sample_rgba8(200), 8u8, 4u8)]
	fn bits_and_channels(#[case] img: DynamicImage, #[case] bits: u8, #[case] chans: u8) {
		assert_eq!(img.bits_per_value(), bits);
		assert_eq!(img.channel_count(), chans);
	}

	// --- extended_color_type & has_alpha ----------------------------------
	#[rstest]
	#[case::l8(sample_l8(), ExtendedColorType::L8, false)]
	#[case::la8(sample_la8(123), ExtendedColorType::La8, true)]
	#[case::rgb8(sample_rgb8(), ExtendedColorType::Rgb8, false)]
	#[case::rgba8(sample_rgba8(42), ExtendedColorType::Rgba8, true)]
	fn color_and_alpha(#[case] img: DynamicImage, #[case] ect: ExtendedColorType, #[case] has_alpha: bool) {
		assert_eq!(img.extended_color_type(), ect);
		assert_eq!(img.has_alpha(), has_alpha);
	}

	// --- is_empty / is_opaque ---------------------------------------------
	#[rstest]
	#[case::l8_opaque(sample_l8(), false, true)]
	#[case::la8_empty(sample_la8(0), true, false)]
	#[case::la8_partial(sample_la8(100), false, false)]
	#[case::la8_opaque(sample_la8(255), false, true)]
	#[case::rgb8_opaque(sample_rgb8(), false, true)]
	#[case::rgba8_empty(sample_rgba8(0), true, false)]
	#[case::rgba8_partial(sample_rgba8(100), false, false)]
	#[case::rgba8_opaque(sample_rgba8(255), false, true)]
	fn empty_and_opaque(#[case] img: DynamicImage, #[case] expect_empty: bool, #[case] expect_opaque: bool) {
		assert_eq!(img.is_empty(), expect_empty);
		assert_eq!(img.is_opaque(), expect_opaque);
	}

	// --- into_optional -----------------------------------------------------
	#[test]
	fn into_optional_behaviour() {
		// Non-empty without alpha stays Some
		let rgb = sample_rgb8();
		assert!(rgb.clone().into_optional().is_some());

		// Empty (all alpha = 0) becomes None
		let rgba_empty = sample_rgba8(0);
		assert!(rgba_empty.into_optional().is_none());

		// Non-empty with alpha stays Some
		let la_opaque = sample_la8(255);
		assert!(la_opaque.into_optional().is_some());
	}

	// --- ensure_same_size / meta ------------------------------------------
	#[test]
	fn ensure_same_size_and_meta_ok() {
		let a = sample_rgb8();
		let b = sample_rgb8();
		a.ensure_same_size(&b).unwrap();
		a.ensure_same_meta(&b).unwrap();
	}

	#[test]
	fn ensure_same_size_error() {
		let a = DynamicImage::from_fn_rgb8(2, 2, |x, y| [x as u8, y as u8, 0]);
		let b = DynamicImage::from_fn_rgb8(3, 2, |x, y| [x as u8, y as u8, 0]);
		let err = a.ensure_same_size(&b).unwrap_err();
		let msg = format!("{err}");
		assert!(msg.contains("width") || msg.contains("height"));
	}

	#[test]
	fn ensure_same_meta_color_mismatch_error() {
		let a = sample_rgb8();
		let b = sample_rgba8(255);
		let err = a.ensure_same_meta(&b).unwrap_err();
		assert!(format!("{err}").contains("Pixel value type mismatch"));
	}

	// --- diff --------------------------------------------------------------
	#[test]
	fn diff_zero_for_identical_images() {
		let a = sample_rgb8();
		let b = sample_rgb8();
		let d = a.diff(&b).unwrap();
		assert_eq!(d, vec![0.0, 0.0, 0.0]);
	}

	#[test]
	fn diff_scales_with_squared_error_and_rounds() {
		// Use a small 2x2 image; change one pixel in one channel by 1.
		let base = DynamicImage::from_fn_rgb8(2, 2, |_, _| [10, 20, 30]);
		let changed = DynamicImage::from_fn_rgb8(2, 2, |_, _| [10, 20, 30]);

		// Manually tweak one pixel's red channel by +1
		// We'll rebuild via from_raw to keep API surface area small
		let mut raw = changed.as_bytes().to_vec();
		// Pixel layout for RGB8 is 3 bytes per pixel; tweak first pixel red channel
		raw[0] = raw[0].saturating_add(1);
		let changed = DynamicImage::from_raw(2, 2, raw).unwrap();

		// For one pixel with delta 1: v = 1^2 = 1, n = 4 -> ceil(10 * 1/4)/10 = 0.3
		let d = base.diff(&changed).unwrap();
		assert_eq!(d.len(), 3);
		assert!((d[0] - 0.3).abs() < f64::EPSILON);
		assert_eq!(d[1], 0.0);
		assert_eq!(d[2], 0.0);
	}
}
