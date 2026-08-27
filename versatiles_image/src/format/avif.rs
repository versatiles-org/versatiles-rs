//! AVIF (AV1 Image File Format) encoder bridges for `DynamicImage`.
//!
//! This module exposes small helpers to encode images into AVIF blobs with configurable
//! **quality** (lossy) and **effort**. Decoding is intentionally **not implemented** here; the
//! rest of the crate treats AVIF as a write-only target for web tile pipelines.
//!
//! Notes:
//! - Only **8‑bit** images are supported; higher bit depths are rejected early.
//! - "Lossless" AVIF (quality ≥ 100) is not supported by the encoder binding used here.
//! - The optional `effort` argument (0–100) is mapped linearly to the encoder range **1..=9**
//!   (1 = slow/best, 9 = fast). Higher effort → slower/better compression. Encoder speed 10 is
//!   deliberately unreachable — see [`encode`].
//! - Pixels are coded in **BT.601 YCbCr, full range, 4:4:4** rather than in the identity (GBR)
//!   color model, which skips chroma decorrelation. Measured on real 256×256 tiles, the switch
//!   (together with quality 90 → 92) cut output by 29% on a rendered map tile, 60% on satellite
//!   imagery and 62% on a greyscale tile, with equal or better structural similarity — per-pixel
//!   PSNR drops ~1.6 dB on flat map tiles, where the loss is chroma rounding at sharp edges.
//!   Channel-encoded data such as terrain-RGB DEM is the one input that prefers the identity
//!   model, but that needs lossless encoding, which this module rejects anyway.

use anyhow::{Result, bail};
use image::{
	DynamicImage, ImageEncoder,
	codecs::avif::{AvifEncoder, ColorSpace},
};
use versatiles_core::Blob;
use versatiles_derive::context;

use crate::traits::DynamicImageTraitInfo;

/// Quality used when the caller passes `None`.
const DEFAULT_QUALITY: u8 = 92;

/// Encoder speed used when the caller passes no `effort`.
const DEFAULT_SPEED: u8 = 8;

/// Map the user-facing `effort` scale (0–100) onto the encoder speed the AVIF encoder wants.
///
/// Effort 0 gives encoder speed 9 (fast), effort 100 gives speed 1 (slow/best), and `None` gives
/// [`DEFAULT_SPEED`].
///
/// Encoder speed 10 is deliberately unreachable: rav1e emits *larger* files there than at speed 9
/// — measured 90.3 KB vs 42.2 KB on a 256×256 satellite tile at quality 90 — so it is dominated on
/// both axes and no effort value should select it.
#[expect(
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss,
	reason = "clamped to 1..=9 on the same line"
)]
fn effort_to_speed(effort: Option<u8>) -> u8 {
	effort.map_or(DEFAULT_SPEED, |e| {
		(9.0 - f32::from(e) / 100.0 * 8.0).round().clamp(1.0, 9.0) as u8
	})
}

/// Encode a `DynamicImage` into an AVIF [`Blob`].
///
/// * `quality` — 0..=99 (higher means better quality & larger size). `None` defaults to **92**.
/// * `effort` — 0..=100 user scale (mapped to encoder 1..=9). `None` defaults to encoder speed
///   **8**, the knee of the size/time curve. Higher effort → slower/better compression.
///
/// Returns an error if the image is not 8‑bit per channel or if `quality >= 100`.
#[context("encoding {}x{} {:?} as AVIF (q={:?}, e={:?})", image.width(), image.height(), image.color(), quality, effort)]
pub fn encode(image: &DynamicImage, quality: Option<u8>, effort: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("avif only supports 8-bit images");
	}

	// 92 rather than 90: YCbCr needs slightly more quality than the identity color model to
	// match it on flat map tiles, and still lands well below it in bytes.
	let quality = quality.unwrap_or(DEFAULT_QUALITY);
	if quality >= 100 {
		bail!("Lossless AVIF encoding is not supported, quality must be less than 100");
	}

	let encoder_speed = effort_to_speed(effort);

	let mut result: Vec<u8> = vec![];
	// `ColorSpace::Bt709` selects `ravif::ColorModel::YCbCr`, which codes BT.601 full-range
	// 4:4:4 — the `image` enum name does not match the matrix ravif actually signals.
	let encoder = AvifEncoder::new_with_speed_quality(&mut result, encoder_speed, quality)
		.with_colorspace(ColorSpace::Bt709)
		.with_num_threads(Some(1));

	encoder.write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.extended_color_type(),
	)?;

	Ok(Blob::from(result))
}

/// Convenience wrapper around [`encode`] with an `effort` chosen automatically (fast).
///
/// Use `quality = None` for the default (92).
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
mod tests {
	use approx::assert_relative_eq;
	use rstest::rstest;

	/// AVIF encoding smoke tests: verify byte‑size ratios for our synthetic patterns
	/// and validate the explicit error for the unsupported "lossless" path.
	use super::*;
	use crate::traits::{DynamicImageTraitConvert, DynamicImageTraitTest};

	/* ---------- Success cases ---------- */
	/// Baselines for encoder speed 8 in the BT.601 YCbCr color model.
	///
	/// The grey cases shrank (2.14 → 0.65 and 1.29 → 0.56): chroma decorrelation costs nothing
	/// on a grey image, while the identity model paid for three real planes. The colour cases
	/// barely moved (0.79 → 0.83 and 0.83 → 0.75) because the two knobs pull against each other
	/// here — the speed fix alone takes them to 0.29 and 0.34, and the colour model gives that
	/// back. These synthetic images are *exact* linear ramps with anti-correlated red and green,
	/// the one pattern the identity model predicts almost for free and the colour conversion
	/// turns into noisy chroma; real map and imagery tiles move the other way, as the module
	/// docs record.
	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), 0.65)]
	#[case::greya(DynamicImage::new_test_greya(), 0.56)]
	#[case::rgb(DynamicImage::new_test_rgb(), 0.83)]
	#[case::rgba(DynamicImage::new_test_rgba(), 0.75)]
	fn avif_ok(#[case] img: DynamicImage, #[case] expected_compression_percent: f64) -> Result<()> {
		let blob = image2blob(&img, None)?;

		let compression_percent = ((10_000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0;
		assert_relative_eq!(compression_percent, expected_compression_percent);

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

	/// `encode` accepts any `effort` in 0..=100 and produces non-empty output.
	///
	/// Effort 100 maps to encoder speed 1 (slowest) and effort 0 to speed 9
	/// (fastest). We cover both boundaries and the midpoint on a single tiny
	/// image to keep the test fast — the underlying AVIF library is expensive
	/// at high effort.
	#[rstest]
	#[case::fastest(0)]
	#[case::middle(50)]
	#[case::slowest(100)]
	fn encode_accepts_valid_effort(#[case] effort: u8) -> Result<()> {
		#[expect(
			clippy::cast_possible_truncation,
			reason = "32x32 test fixture; x and y stay below 256"
		)]
		let img = DynamicImage::from_fn(32, 32, |x, y| [x as u8, (255 - x) as u8, y as u8]);
		let blob = encode(&img, Some(80), Some(effort))?;
		assert!(!blob.is_empty());
		Ok(())
	}

	/// The effort scale must never select encoder speed 10.
	///
	/// rav1e's speed 10 is dominated: it is faster than speed 9 but also *bigger* (90.3 KB vs
	/// 42.2 KB on a 256×256 satellite tile at quality 90), so it must stay unreachable however
	/// the mapping is later rewritten.
	#[test]
	fn effort_never_selects_speed_10() {
		assert_eq!(effort_to_speed(None), DEFAULT_SPEED);
		assert_eq!(effort_to_speed(Some(0)), 9);
		assert_eq!(effort_to_speed(Some(100)), 1);
		for e in 0..=u8::MAX {
			let speed = effort_to_speed(Some(e));
			assert!((1..=9).contains(&speed), "effort {e} mapped to speed {speed}");
		}
	}

	/// Effort is monotonic: more effort never means a faster encoder setting.
	#[test]
	fn effort_is_monotonic() {
		let speeds: Vec<u8> = (0..=100).map(|e| effort_to_speed(Some(e))).collect();
		assert!(speeds.windows(2).all(|w| w[0] >= w[1]), "{speeds:?}");
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
