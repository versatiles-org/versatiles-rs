//! High-level image operations for `DynamicImage` used in VersaTiles.
//!
//! This module provides [`DynamicImageTraitOperation`], which augments `image::DynamicImage` with
//! pragmatic helpers for common tile-processing tasks:
//!
//! - Removing alpha channels (unconditionally or only if fully opaque)
//! - Computing a quick representative/average color
//! - Cropping with resampling and downscaling with configurable filters
//! - Alpha-aware flattening against a background color
//! - In-place mutation of color channels (leaving alpha intact)
//! - In-place overlay compositing of two images with size validation
//!
//! Most operations use efficient backends (`fast_image_resize` where applicable) and avoid
//! unnecessary allocations.

use super::info::DynamicImageTraitInfo;
use anyhow::{Result, bail};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, Rgb, imageops::overlay};
use imageproc::map::map_pixels;
use versatiles_derive::context;

/// High-level convenience operations for modifying and transforming `DynamicImage`s.
pub trait DynamicImageTraitOperation: DynamicImageTraitInfo {
	/// Returns a copy of the image **without** an alpha channel.
	///
	/// * `Rgba8` → `Rgb8`, `La8` → `L8`. Non‑alpha images are returned unchanged.
	/// * Returns an error for unsupported color types.
	fn as_no_alpha(&self) -> Result<DynamicImage>;

	/// Computes a quick **representative color** of the image.
	///
	/// This downsamples to 1×1 using a triangle filter and returns the resulting pixel bytes
	/// (3 or 4 channels depending on the source color type).
	fn average_color(&self) -> Vec<u8>;

	/// Crops the source region `(x, y, w, h)` and resamples it into a destination image of
	/// size `width_dst × height_dst` using `fast_image_resize`.
	///
	/// Coordinates are given in source pixel space. Returns an error on resize failures.
	fn get_extract(&self, x: f64, y: f64, w: f64, h: f64, width_dst: u32, height_dst: u32) -> Result<DynamicImage>;

	/// Produces a scaled‑down copy by the integer `factor` using a **box filter**.
	///
	/// Panics if `factor == 0`. Returns an error on resize failures.
	fn get_scaled_down(&self, factor: u32) -> Result<DynamicImage>;

	/// Flattens an image with alpha against a given **background color**.
	///
	/// For `Rgba8`: returns an `Rgb8` where each pixel is `alpha * src + (1-alpha) * bg`.
	/// Non‑alpha images are returned unchanged. Errors on unsupported color types.
	fn into_flattened(self, color: Rgb<u8>) -> Result<DynamicImage>;

	/// Converts the image to a non‑alpha variant **only if it is fully opaque**.
	///
	/// For non‑alpha images this is a no‑op. Errors for unsupported color types.
	fn into_no_alpha_if_opaque(self) -> Result<DynamicImage>;

	/// Unconditionally removes the alpha channel (if present) and returns the result.
	///
	/// Non‑alpha images are returned unchanged. Errors for unsupported color types.
	fn into_no_alpha(self) -> Result<DynamicImage>;

	/// Like [`get_scaled_down`](Self::get_scaled_down), but consumes `self` and returns the result.
	///
	/// Returns `self` unchanged when `factor == 1`.
	fn into_scaled_down(self, factor: u32) -> Result<DynamicImage>;

	/// Sets **all alpha values to 255** in place, making the image fully opaque.
	///
	/// Has no effect on images without alpha. Errors on unsupported color types.
	fn make_opaque(&mut self) -> Result<()>;

	/// Applies a mapping function `f` to **color channels only**, leaving the alpha channel intact.
	///
	/// The function is called per component (`u8`). Errors on unsupported color types.
	fn mut_color_values<F>(&mut self, f: F)
	where
		F: Fn(u8) -> u8;

	/// Draws `top` over `self` in place (composites at (0,0)).
	///
	/// Ensures the images have identical dimensions. Returns an error if sizes differ.
	fn overlay(&mut self, top: &DynamicImage) -> Result<()>;
}

impl DynamicImageTraitOperation for DynamicImage
where
	DynamicImage: DynamicImageTraitInfo,
{
	#[context("removing alpha from {:?} image (has_alpha={})", self.color(), self.has_alpha())]
	fn as_no_alpha(&self) -> Result<DynamicImage> {
		Ok(match self {
			DynamicImage::ImageRgba8(_) => DynamicImage::from(self.to_rgb8()),
			DynamicImage::ImageLumaA8(_) => DynamicImage::from(self.to_luma8()),
			DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => self.clone(),
			_ => bail!("Unsupported image type for removing alpha: {:?}", self.color()),
		})
	}

	fn average_color(&self) -> Vec<u8> {
		let img = self.resize_exact(1, 1, image::imageops::FilterType::Triangle);
		img.into_bytes()
	}

	#[context("extracting region ({:.2},{:.2},{:.2},{:.2}) from {}x{} into {}x{}", x, y, w, h, self.width(), self.height(), width_dst, height_dst)]
	fn get_extract(&self, x: f64, y: f64, w: f64, h: f64, width_dst: u32, height_dst: u32) -> Result<DynamicImage> {
		let mut dst_image = DynamicImage::new(width_dst, height_dst, self.color());
		Resizer::new().resize(self, &mut dst_image, &ResizeOptions::default().crop(x, y, w, h))?;
		Ok(dst_image)
	}

	#[context("downscaling {}x{} by factor {} ({:?})", self.width(), self.height(), factor, self.color())]
	fn get_scaled_down(&self, factor: u32) -> Result<DynamicImage> {
		assert!(factor > 0, "Scaling factor must be greater than zero");

		let mut dst_image = DynamicImage::new(self.width() / factor, self.height() / factor, self.color());
		Resizer::new().resize(
			self,
			&mut dst_image,
			&ResizeOptions::default().resize_alg(ResizeAlg::Convolution(FilterType::Box)),
		)?;

		Ok(dst_image)
	}

	#[context("flattening image onto RGB({:?})", color)]
	#[allow(clippy::cast_possible_truncation)]
	fn into_flattened(self, color: Rgb<u8>) -> Result<DynamicImage> {
		if !self.has_alpha() {
			return Ok(self);
		}
		match self {
			DynamicImage::ImageRgba8(img) => {
				let c = [u16::from(color[0]), u16::from(color[1]), u16::from(color[2])];
				Ok(DynamicImage::from(map_pixels(&img, |p| {
					if p[3] == 255 {
						Rgb([p[0], p[1], p[2]])
					} else {
						let a = u16::from(p[3]);
						let b = u16::from(255 - p[3]);
						Rgb([
							(((u16::from(p[0]) * a) + c[0] * b + 127) / 255) as u8,
							(((u16::from(p[1]) * a) + c[1] * b + 127) / 255) as u8,
							(((u16::from(p[2]) * a) + c[2] * b + 127) / 255) as u8,
						])
					}
				})))
			}
			_ => bail!("Unsupported image type {:?} for flattening", self.color()),
		}
	}

	#[context("dropping alpha from image")]
	fn into_no_alpha(self) -> Result<DynamicImage> {
		Ok(match self {
			DynamicImage::ImageRgba8(_) => DynamicImage::from(self.into_rgb8()),
			DynamicImage::ImageLumaA8(_) => DynamicImage::from(self.into_luma8()),
			DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => self,
			_ => bail!("Unsupported image type for removing alpha: {:?}", self.color()),
		})
	}

	#[context("removing alpha if opaque")]
	fn into_no_alpha_if_opaque(self) -> Result<DynamicImage> {
		if self.has_alpha() && self.is_opaque() {
			self.into_no_alpha()
		} else {
			Ok(self)
		}
	}

	#[context("downscaling image by factor {factor}")]
	fn into_scaled_down(self, factor: u32) -> Result<DynamicImage> {
		if factor == 1 {
			Ok(self)
		} else {
			self.get_scaled_down(factor)
		}
	}

	#[context("forcing opacity for {:?} image", self.color())]
	fn make_opaque(&mut self) -> Result<()> {
		match *self {
			DynamicImage::ImageRgba8(ref mut img) => {
				for p in img.pixels_mut() {
					p[3] = 255;
				}
			}
			DynamicImage::ImageLumaA8(ref mut img) => {
				for p in img.pixels_mut() {
					p[1] = 255;
				}
			}
			DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => {}
			_ => bail!("Unsupported image type for removing alpha: {:?}", self.color()),
		}
		Ok(())
	}

	fn mut_color_values<F>(&mut self, f: F)
	where
		F: Fn(u8) -> u8,
	{
		match self {
			DynamicImage::ImageLuma8(img) => {
				for p in img.pixels_mut() {
					p[0] = f(p[0]);
				}
			}
			DynamicImage::ImageLumaA8(img) => {
				for p in img.pixels_mut() {
					p[0] = f(p[0]);
				}
			}
			DynamicImage::ImageRgb8(img) => {
				for p in img.pixels_mut() {
					p[0] = f(p[0]);
					p[1] = f(p[1]);
					p[2] = f(p[2]);
				}
			}
			DynamicImage::ImageRgba8(img) => {
				for p in img.pixels_mut() {
					p[0] = f(p[0]);
					p[1] = f(p[1]);
					p[2] = f(p[2]);
				}
			}
			_ => panic!("Unsupported image type for mutating color values: {:?}", self.color()),
		}
	}

	#[context("overlaying top {}x{} {:?} onto base {}x{} {:?}", top.width(), top.height(), top.color(), self.width(), self.height(), self.color())]
	fn overlay(&mut self, top: &DynamicImage) -> Result<()> {
		self.ensure_same_size(top)?;
		overlay(self, top, 0, 0);
		Ok(())
	}
}

/// Tests: alpha handling (drop/flatten/opaque), average color, scaling/cropping, overlays, and
/// color‑channel mutations using `rstest` parameterization where appropriate.
#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::traits::*;
	use image::ExtendedColorType as ECT;
	use image::{GenericImageView, Pixel, Rgba};
	use rstest::rstest;

	#[rstest]
	#[case::rgba(DynamicImage::new_test_rgba(), ECT::Rgb8, false)]
	#[case::la(DynamicImage::new_test_greya(), ECT::L8, false)]
	#[case::rgb(DynamicImage::new_test_rgb(), ECT::Rgb8, false)]
	#[case::grey(DynamicImage::new_test_grey(), ECT::L8, false)]
	fn as_no_alpha_drops_alpha_when_present(
		#[case] src: DynamicImage,
		#[case] expect_type: ECT,
		#[case] expect_has_alpha: bool,
	) {
		let out = src.as_no_alpha().unwrap();
		assert_eq!(out.extended_color_type(), expect_type);
		assert_eq!(out.has_alpha(), expect_has_alpha);
	}

	#[rstest]
	#[case::rgba(DynamicImage::new_test_rgba(), ECT::Rgb8)]
	#[case::la(DynamicImage::new_test_greya(), ECT::L8)]
	#[case::rgb(DynamicImage::new_test_rgb(), ECT::Rgb8)]
	#[case::grey(DynamicImage::new_test_grey(), ECT::L8)]
	fn into_no_alpha_variants(#[case] src: DynamicImage, #[case] expect_type: ECT) {
		let out = src.into_no_alpha().unwrap();
		assert_eq!(out.extended_color_type(), expect_type);
		assert!(!out.has_alpha());
	}

	#[test]
	fn average_color_on_solid_rgb_is_exact() {
		// Solid color should average to itself exactly (filtering can't change a constant)
		let img = DynamicImage::from_fn(11, 11, |x, y| {
			[100 - x as u8, 110 - y as u8, 120 + x as u8, 130 + y as u8]
		});
		assert_eq!(img.average_color(), [95, 105, 125, 135]);
	}

	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(),&[128])]
	#[case::greya(DynamicImage::new_test_greya(),&[128,128])]
	#[case::rgb(DynamicImage::new_test_rgb(),&[128, 127, 128])]
	#[case::rgba(DynamicImage::new_test_rgba(),&[128, 127, 128, 127])]
	fn average_color_on_gradients_is_centerish(#[case] img: DynamicImage, #[case] expect: &[u8]) {
		assert_eq!(img.average_color(), expect);
	}

	#[rstest]
	#[case::rgba(DynamicImage::new_test_rgba(), Some((4usize, 3usize)))]
	#[case::la(DynamicImage::new_test_greya(), Some((2usize, 1usize)))]
	#[case::rgb(DynamicImage::new_test_rgb(), None)]
	#[case::luma(DynamicImage::new_test_grey(), None)]
	fn make_opaque_behaviour(
		#[case] mut img: DynamicImage,
		#[case] alpha_layout: Option<(usize, usize)>, // (stride, alpha_index)
	) {
		let before = img.as_bytes().to_vec();
		let has_alpha_before = img.has_alpha();

		img.make_opaque().unwrap();

		// Always opaque afterwards
		assert!(img.is_opaque());
		let after = img.as_bytes();

		if let Some((stride, aidx)) = alpha_layout {
			assert!(has_alpha_before, "expected an alpha channel");
			// Color bytes unchanged; alpha bytes set to 255
			for (i, (&a, &b)) in after.iter().zip(before.iter()).enumerate() {
				if i % stride == aidx {
					assert_eq!(a, 255, "alpha not set to 255 at byte index {i}");
				} else {
					assert_eq!(a, b, "color byte changed at index {i}");
				}
			}
			// Color type should remain the same (still has an alpha channel)
			assert!(img.has_alpha());
		} else {
			assert!(!has_alpha_before, "did not expect an alpha channel");
			// No-op for images without alpha: data unchanged
			assert_eq!(after, &before[..]);
			assert!(img.is_opaque());
		}
	}

	#[rstest]
	#[case::la(DynamicImage::new_test_greya(), ECT::La8, true, ECT::L8, false)]
	#[case::luma(DynamicImage::new_test_grey(), ECT::L8, false, ECT::L8, false)]
	#[case::rgb(DynamicImage::new_test_rgb(), ECT::Rgb8, false, ECT::Rgb8, false)]
	#[case::rgba(DynamicImage::new_test_rgba(), ECT::Rgba8, true, ECT::Rgb8, false)]
	fn into_no_alpha_if_opaque_behaviour(
		#[case] img: DynamicImage,
		#[case] expect_type_nonopaque: ECT,
		#[case] expect_has_alpha_nonopaque: bool,
		#[case] expect_type_opaque: ECT,
		#[case] expect_has_alpha_opaque: bool,
	) {
		// First: when image is not made opaque
		let out1 = img.clone().into_no_alpha_if_opaque().unwrap();
		assert_eq!(out1.extended_color_type(), expect_type_nonopaque);
		assert_eq!(out1.has_alpha(), expect_has_alpha_nonopaque);

		// Second: after forcing opacity (no-op for non-alpha images)
		let mut opaque_img = img.clone();
		opaque_img.make_opaque().unwrap();
		let out2 = opaque_img.into_no_alpha_if_opaque().unwrap();
		assert_eq!(out2.extended_color_type(), expect_type_opaque);
		assert_eq!(out2.has_alpha(), expect_has_alpha_opaque);
	}

	#[rstest]
	// No alpha: all bytes should become 0
	#[case::luma(DynamicImage::new_test_grey(), None)]
	#[case::rgb(DynamicImage::new_test_rgb(), None)]
	// With alpha: color bytes -> 0, alpha bytes unchanged
	#[case::luma_a(DynamicImage::new_test_greya(), Some((2usize, 1usize)))] // stride=2, alpha at index 1
	#[case::rgba(DynamicImage::new_test_rgba(), Some((4usize, 3usize)))] // stride=4, alpha at index 3
	fn mut_color_values_applies_fn_to_all_color_channels(
		#[case] mut img: DynamicImage,
		#[case] alpha_layout: Option<(usize, usize)>, // (stride, alpha_index)
	) {
		let before = img.as_bytes().to_vec();
		img.mut_color_values(|_| 0);
		let after = img.as_bytes();

		match alpha_layout {
			None => {
				// Luma8/Rgb8: everything zeroed
				assert!(after.iter().all(|&b| b == 0));
			}
			Some((stride, aidx)) => {
				// LumaA8/Rgba8: only alpha bytes preserved, colors zeroed
				for (i, (&a, &b)) in after.iter().zip(before.iter()).enumerate() {
					if i % stride == aidx {
						assert_eq!(a, b, "alpha channel changed at index {i}");
					} else {
						assert_eq!(a, 0, "color channel not zeroed at index {i}");
					}
				}
			}
		}
	}

	#[rstest]
	#[case::black(Rgba([0, 0, 0, 255]))]
	#[case::white(Rgba([255, 255, 255, 255]))]
	fn into_flattened_blends_with_background_when_alpha_present(#[case] bg: Rgba<u8>) {
		// Pattern: RGBA = [x, 255-x, y, 255-y]
		let rgba = DynamicImage::new_test_rgba();
		let flat = rgba.clone().into_flattened(bg.to_rgb()).unwrap();
		assert_eq!(flat.extended_color_type(), ECT::Rgb8);
		assert!(!flat.has_alpha());

		// Pixel at (10, 0): alpha = 255 -> unchanged
		let (x, y) = (10u32, 0u32);
		let p_src = rgba.get_pixel(x, y).0;
		let p_dst = flat.get_pixel(x, y).0;
		assert_eq!(&p_dst, &p_src);

		// Pixel at (20, 255): alpha = 0 -> becomes background color
		let (x, y) = (20u32, 255u32);
		let p_dst = flat.get_pixel(x, y).0;
		assert_eq!(p_dst, bg.0);
	}

	#[rstest]
	#[case::factor_1(1, (256, 256))]
	#[case::factor_2(2, (128, 128))]
	#[case::factor_4(4, (64, 64))]
	fn get_scaled_down_reduces_dimensions(#[case] factor: u32, #[case] expect_dims: (u32, u32)) {
		let img = DynamicImage::new_test_rgb();
		let out = img.clone().into_scaled_down(factor).unwrap();
		assert_eq!(out.dimensions(), expect_dims);
		let out2 = img.get_scaled_down(factor).unwrap();
		assert_eq!(out2.dimensions(), expect_dims);
	}

	#[test]
	fn get_extract_returns_requested_size() {
		let img = DynamicImage::new_test_rgb();
		// Crop a centered 128x128 region and request same-sized output
		let out = img.get_extract(64.0, 64.0, 128.0, 128.0, 128, 128).unwrap();
		assert_eq!(out.dimensions(), (128, 128));
		assert_eq!(out.extended_color_type(), ECT::Rgb8);
	}

	#[test]
	fn overlay_draws_top_over_bottom() {
		// Bottom: black RGB 16x16
		let mut bottom = DynamicImage::from_fn(16, 16, |_x, _y| [0, 0, 0]);
		// Top: solid red RGB 16x16
		let top = DynamicImage::from_fn(16, 16, |_x, _y| [255, 0, 0]);

		bottom.overlay(&top).unwrap();
		// A few sample pixels should now be red
		for &(x, y) in &[(0, 0), (8, 8), (15, 15)] {
			assert_eq!(bottom.get_pixel(x, y).0, [255, 0, 0, 255]);
		}
	}
}
