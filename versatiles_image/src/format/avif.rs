//! AVIF (AV1 Image File Format) encoder bridges for `DynamicImage`.
//!
//! This module exposes small helpers to encode images into AVIF blobs with configurable
//! **quality** (lossy) and **speed**. Decoding is intentionally **not implemented** here; the
//! rest of the crate treats AVIF as a write-only target for web tile pipelines.
//!
//! Notes:
//! - Only **8‑bit** images are supported; higher bit depths are rejected early.
//! - "Lossless" AVIF (quality ≥ 100) is not supported by the encoder binding used here.
//! - The optional `speed` argument (0–100) is mapped linearly to the encoder range **1..=10**
//!   (1 = slow/best, 10 = fast).

use crate::traits::DynamicImageTraitInfo;
use anyhow::{Result, bail};
use image::{
	DynamicImage, ImageEncoder,
	codecs::avif::{AvifEncoder, ColorSpace},
};
use versatiles_core::Blob;
use versatiles_derive::context;

/// Encode a `DynamicImage` into an AVIF [`Blob`].
///
/// * `quality` — 0..=99 (higher means better quality & larger size). `None` defaults to **90**.
/// * `speed` — 0..=100 user scale (mapped to encoder 1..=10). `None` defaults to **10** (fastest).
///
/// Returns an error if the image is not 8‑bit per channel or if `quality >= 100`.
#[context("encoding {}x{} {:?} as AVIF (q={:?}, s={:?})", image.width(), image.height(), image.color(), quality, speed)]
pub fn encode(image: &DynamicImage, quality: Option<u8>, speed: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("avif only supports 8-bit images");
	}

	let quality = quality.unwrap_or(90);
	if quality >= 100 {
		bail!("Lossless AVIF encoding is not supported, quality must be less than 100");
	}

	#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
	// speed 1..=10 fits into u8, clamp ensures positive
	let speed = speed.map_or(10, |s| {
		(f32::from(s) / 100.0 * 9.0 + 1.0).round().clamp(1.0, 10.0) as u8
	});

	let mut result: Vec<u8> = vec![];
	let encoder = AvifEncoder::new_with_speed_quality(&mut result, speed, quality)
		.with_colorspace(ColorSpace::Srgb)
		.with_num_threads(Some(1));

	encoder.write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.extended_color_type(),
	)?;

	Ok(Blob::from(result))
}

/// Convenience wrapper around [`encode`] with a `speed` chosen automatically (fast).
///
/// Use `quality = None` for the default (90).
#[context("encoding image {:?} as AVIF (q={:?})", image.color(), quality)]
pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	encode(image, quality, None)
}

/// Attempt a so‑called "lossless" AVIF export.
///
/// This always returns an error, documenting the limitation that our encoder does not
/// support `quality >= 100`. Kept as an explicit function to make call‑sites self‑documenting.
#[context("encoding image {:?} as 'lossless' AVIF", image.color())]
pub fn image2blob_lossless(image: &DynamicImage) -> Result<Blob> {
	encode(image, Some(100), None)
}

/// AVIF decoding is **not implemented** in this crate.
///
/// Returned error explains the rationale; callers should decode via another backend if needed.
#[context("decoding AVIF blob ({} bytes)", _blob.len())]
pub fn blob2image(_blob: &Blob) -> Result<DynamicImage> {
	bail!("AVIF decoding not implemented")
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	/// AVIF encoding smoke tests: verify byte‑size ratios for our synthetic patterns
	/// and validate the explicit error for the unsupported "lossless" path.
	use super::*;
	use crate::traits::DynamicImageTraitTest;
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), 1.99)]
	#[case::greya(DynamicImage::new_test_greya(), 1.23)]
	#[case::rgb(DynamicImage::new_test_rgb(), 0.58)]
	#[case::rgba(DynamicImage::new_test_rgba(), 0.55)]
	fn avif_ok(#[case] img: DynamicImage, #[case] expected_compression_percent: f64) -> Result<()> {
		let blob = image2blob(&img, None)?;

		let compression_percent = ((10_000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0;
		assert_eq!(compression_percent, expected_compression_percent);

		Ok(())
	}

	#[rstest]
	#[case::grey(DynamicImage::new_test_grey())]
	#[case::greya(DynamicImage::new_test_greya())]
	#[case::rgb(DynamicImage::new_test_rgb())]
	#[case::rgba(DynamicImage::new_test_rgba())]
	fn avif_lossless_ok(#[case] img: DynamicImage) -> Result<()> {
		assert_eq!(
			image2blob_lossless(&img)
				.unwrap_err()
				.chain()
				.last()
				.unwrap()
				.to_string(),
			"Lossless AVIF encoding is not supported, quality must be less than 100"
		);

		Ok(())
	}

	/* ---------- encode() direct tests ---------- */

	#[test]
	fn encode_with_custom_quality() -> Result<()> {
		let img = DynamicImage::new_test_rgb();
		let blob_q50 = encode(&img, Some(50), None)?;
		let blob_q90 = encode(&img, Some(90), None)?;
		// Lower quality should generally produce smaller files
		assert!(blob_q50.len() < blob_q90.len());
		Ok(())
	}

	#[test]
	fn encode_with_custom_speed() -> Result<()> {
		let img = DynamicImage::new_test_rgb();
		// Speed 0 maps to encoder speed 1 (slowest)
		let blob_slow = encode(&img, Some(80), Some(0))?;
		// Speed 100 maps to encoder speed 10 (fastest)
		let blob_fast = encode(&img, Some(80), Some(100))?;
		// Both should produce valid output
		assert!(!blob_slow.is_empty());
		assert!(!blob_fast.is_empty());
		Ok(())
	}

	#[test]
	fn encode_speed_edge_cases() -> Result<()> {
		let img = DynamicImage::new_test_rgb();
		// Test speed boundaries
		assert!(encode(&img, Some(80), Some(0)).is_ok());
		assert!(encode(&img, Some(80), Some(50)).is_ok());
		assert!(encode(&img, Some(80), Some(100)).is_ok());
		Ok(())
	}

	#[test]
	fn encode_quality_boundary() {
		let img = DynamicImage::new_test_rgb();
		// quality 99 should work
		assert!(encode(&img, Some(99), None).is_ok());
		// quality 100 should fail
		assert!(encode(&img, Some(100), None).is_err());
	}

	/* ---------- Error cases ---------- */

	#[test]
	fn encode_non_8bit_image_fails() {
		use image::{ImageBuffer, Rgb};
		// Create a 16-bit RGB image
		let img16: ImageBuffer<Rgb<u16>, Vec<u16>> = ImageBuffer::new(8, 8);
		let dynamic_img = DynamicImage::from(img16);
		let result = encode(&dynamic_img, None, None);
		assert!(result.is_err());
		let err_msg = result.unwrap_err().chain().last().unwrap().to_string();
		assert!(err_msg.contains("8-bit"), "Expected '8-bit' in: {err_msg}");
	}

	#[test]
	fn blob2image_not_implemented() {
		let blob = Blob::from(vec![1, 2, 3]);
		let result = blob2image(&blob);
		assert!(result.is_err());
		let err_msg = result.unwrap_err().chain().last().unwrap().to_string();
		assert!(
			err_msg.contains("AVIF decoding not implemented"),
			"Expected 'AVIF decoding not implemented' in: {err_msg}"
		);
	}
}
