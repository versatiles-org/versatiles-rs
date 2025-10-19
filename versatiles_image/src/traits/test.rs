//! Helper trait and utilities for generating synthetic test images used across the `versatiles_image` crate.
//!
//! This module defines [`DynamicImageTraitTest`], which extends `image::DynamicImage` with
//! convenience constructors that generate deterministic, 256×256 test patterns.
//! These functions are used for unit testing encoding, decoding, and pixel-processing utilities.

use super::convert::DynamicImageTraitConvert;
use image::DynamicImage;

/// Provides factory functions for generating reproducible gradient-based test images.
/// These are useful for validating conversions, encoders, and format roundtrips.
pub trait DynamicImageTraitTest: DynamicImageTraitConvert {
	/// Generates a 256×256 image with **RGBA** channels.
	/// Red increases with x, green decreases with x, blue increases with y, and alpha decreases with y.
	fn new_test_rgba() -> DynamicImage;

	/// Generates a 256×256 image with **RGB** channels.
	/// Red increases with x, green decreases with x, and blue increases with y.
	fn new_test_rgb() -> DynamicImage;

	/// Generates a 256×256 **grayscale** image.
	/// The brightness increases linearly along the x-axis (from black to white).
	fn new_test_grey() -> DynamicImage;

	/// Generates a 256×256 **grayscale + alpha (LA8)** image.
	/// The luminance increases with x, and the alpha increases with y.
	fn new_test_greya() -> DynamicImage;

	fn new_marker<const N: usize>(parameters: &[(f64, f64); N]) -> DynamicImage;
}

impl DynamicImageTraitTest for DynamicImage
where
	DynamicImage: DynamicImageTraitConvert,
{
	fn new_test_rgba() -> DynamicImage {
		DynamicImage::from_fn_rgba8(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
	}

	fn new_test_rgb() -> DynamicImage {
		DynamicImage::from_fn_rgb8(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8])
	}

	fn new_test_grey() -> DynamicImage {
		DynamicImage::from_fn_l8(256, 256, |x, _y| [x as u8])
	}

	fn new_test_greya() -> DynamicImage {
		DynamicImage::from_fn_la8(256, 256, |x, y| [x as u8, y as u8])
	}

	fn new_marker<const N: usize>(parameters: &[(f64, f64); N]) -> DynamicImage {
		fn f<const N: usize>(x: u32, y: u32, vector: &[(f64, f64); N]) -> [u8; N] {
			let xf = f64::from(x) / 255.0 - 0.5;
			let yf = f64::from(y) / 255.0 - 0.5;
			vector.map(|vector| (vector.0 * xf + vector.1 * yf).round().clamp(0.0, 255.0) as u8)
		}
		let v = parameters
			.iter()
			.map(|p| (p.0.cos() * p.1, p.0.sin() * p.1))
			.collect::<Vec<(f64, f64)>>();

		use DynamicImage as D;
		match v.as_slice() {
			[a] => D::from_fn_l8(256, 256, |x, y| f(x, y, &[*a])),
			[a, b] => D::from_fn_la8(256, 256, |x, y| f(x, y, &[*a, *b])),
			[a, b, c] => D::from_fn_rgb8(256, 256, |x, y| f(x, y, &[*a, *b, *c])),
			[a, b, c, d] => D::from_fn_rgba8(256, 256, |x, y| f(x, y, &[*a, *b, *c, *d])),
			_ => {
				panic!("Marker generation only supports 1 to 4 channels, got {}", v.len());
			}
		}
	}
}

/// Unit tests verifying pixel gradients and expected value patterns for each synthetic image.
/// The test compares selected pixel values (0, 128, 255) to symbolic representations for clarity.
#[cfg(test)]
mod tests {
	use super::*;
	use image::GenericImageView;
	use rstest::rstest;

	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), [
		"...# +++# ####",
		"...# +++# ####",
		"...# +++# ####"
	])]
	#[case::greya(DynamicImage::new_test_greya(), [
		".... +++. ###.",
		"...+ ++++ ###+",
		"...# +++# ####"
	])]
	#[case::rgb(DynamicImage::new_test_rgb(), [
		".#.# ++.# #..#",
		".#+# +++# #.+#",
		".### ++## #.##"
	])]
	#[case::rgba(DynamicImage::new_test_rgba(), [
		".#.# ++.# #..#",
		".#++ ++++ #.++",
		".##. ++#. #.#."
	])]
	fn check_dimensions_and_gradients(#[case] img: DynamicImage, #[case] colors: [&str; 3]) {
		assert_eq!(img.dimensions(), (256, 256));
		let get_pixel = |x: u32, y: u32| {
			img.get_pixel(x, y)
				.0
				.iter()
				.map(|v| match v {
					0 => '.',
					127 | 128 => '+',
					255 => '#',
					_ => panic!("unexpected value {v}"),
				})
				.collect::<String>()
		};
		let colors_result = [
			[get_pixel(0, 0), get_pixel(128, 0), get_pixel(255, 0)].join(" "),
			[get_pixel(0, 128), get_pixel(128, 128), get_pixel(255, 128)].join(" "),
			[get_pixel(0, 255), get_pixel(128, 255), get_pixel(255, 255)].join(" "),
		];
		assert_eq!(colors_result, colors);
	}
}
