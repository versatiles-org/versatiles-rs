//! WebP encoder/decoder utilities for `DynamicImage`.
//!
//! This module bridges the [`image`] crate’s WebP codecs and the internal [`Blob`] type used by
//! VersaTiles. It supports both **lossy** and **lossless** WebP. For lossy mode the `quality`
//! parameter controls the libwebp encoder; for lossless mode pass `quality >= 100`.
//!
//! Highlights:
//! - Only **8‑bit** images are supported.
//! - Accepted layouts for encoding: **RGB8** and **RGBA8**. Greyscale variants are rejected by
//!   design to keep code paths explicit.
//! - When an image has an alpha channel but is **fully opaque**, the encoder **drops alpha** before
//!   encoding to reduce size.
//! - Lossless path uses `image::codecs::webp::WebPEncoder::new_lossless`.
//! - Lossy path uses `libwebp` via `webp::Encoder`.

use crate::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};
use anyhow::{Result, anyhow, bail};
use image::{DynamicImage, ImageFormat, codecs::webp::WebPEncoder, load_from_memory_with_format};
use std::vec;
use versatiles_core::Blob;

/// Encode a `DynamicImage` into a WebP [`Blob`].
///
/// * `quality` — `Some(q)` selects **lossy** encoding for `q < 100` (0..=99), or **lossless** when
///   `q >= 100`. `None` defaults to **95** (lossy).
/// * Only 8‑bit `Rgb8`/`Rgba8` are accepted. If the input has an alpha channel but is fully opaque,
///   alpha is removed first.
///
/// Returns an error for unsupported bit depth or color type.
pub fn encode(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("webp only supports 8-bit images");
	}

	if (image.channel_count() != 3) && (image.channel_count() != 4) {
		bail!("webp only supports RGB or RGBA images");
	}

	let mut image_ref = image;
	#[allow(unused_assignments)]
	let mut optional_image: Option<DynamicImage> = None;
	if image.has_alpha() && image.is_opaque() {
		let i = image.as_no_alpha()?;
		optional_image = Some(i);
		image_ref = optional_image.as_ref().unwrap();
	}

	let quality = quality.unwrap_or(95);

	if quality >= 100 {
		let mut result: Vec<u8> = vec![];
		let encoder = WebPEncoder::new_lossless(&mut result);
		encoder.encode(
			image_ref.as_bytes(),
			image_ref.width(),
			image_ref.height(),
			image_ref.extended_color_type(),
		)?;
		Ok(Blob::from(result))
	} else {
		let encoder = webp::Encoder::from_image(image_ref).map_err(|e| anyhow!("{e}"))?;
		Ok(Blob::from(
			encoder
				.encode_simple(false, f32::from(quality))
				.map_err(|e| anyhow!("{e:?}"))?
				.to_vec(),
		))
	}
}

/// Convenience wrapper around [`encode`].
///
/// `quality = None` uses a default lossy quality of **95**.
pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	encode(image, quality)
}

/// Convenience wrapper for **lossless** WebP encoding (equivalent to `encode(image, Some(100))`).
pub fn image2blob_lossless(image: &DynamicImage) -> Result<Blob> {
	encode(image, Some(100))
}

/// Decode a WebP [`Blob`] back into a [`DynamicImage`].
///
/// Returns a decoding error if the blob is not valid WebP.
pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::WebP)
		.map_err(|e| anyhow!("Failed to decode WebP image: {e}"))
}

#[cfg(test)]
mod tests {
	/// WebP tests: lossy & lossless success cases, rejection of grey/greya inputs,
	/// and verification that fully opaque RGBA is stored without alpha.
	use super::*;
	use crate::traits::DynamicImageTraitTest;
	use rstest::rstest;

	#[rstest]
	#[case::rgb(          DynamicImage::new_test_rgb(),  false, 0.96, vec![0.9, 0.5, 1.5]     )]
	#[case::rgba(         DynamicImage::new_test_rgba(), false, 0.76, vec![0.9, 0.5, 1.6, 0.0])]
	#[case::lossless_rgb( DynamicImage::new_test_rgb(),  true,  0.08, vec![0.0, 0.0, 0.0]     )]
	#[case::lossless_rgba(DynamicImage::new_test_rgba(), true,  0.07, vec![0.0, 0.0, 0.0, 0.0])]
	fn webp_ok(
		#[case] img: DynamicImage,
		#[case] lossless: bool,
		#[case] expected_compression_percent: f64,
		#[case] expected_diff: Vec<f64>,
	) -> Result<()> {
		let blob = if lossless {
			image2blob_lossless(&img)?
		} else {
			image2blob(&img, None)?
		};

		assert_eq!(img.diff(&blob2image(&blob)?)?, expected_diff);

		assert_eq!(
			((10000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0,
			expected_compression_percent
		);

		Ok(())
	}

	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), false, "webp only supports RGB or RGBA images")]
	#[case::greya(DynamicImage::new_test_greya(), false, "webp only supports RGB or RGBA images")]
	#[case::lossless_grey(DynamicImage::new_test_grey(), true, "webp only supports RGB or RGBA images")]
	#[case::lossless_greya(DynamicImage::new_test_greya(), true, "webp only supports RGB or RGBA images")]
	fn webp_errors(#[case] img: DynamicImage, #[case] lossless: bool, #[case] expected_msg: &str) {
		let res = if lossless {
			image2blob_lossless(&img)
		} else {
			image2blob(&img, None)
		};
		assert_eq!(res.unwrap_err().to_string(), expected_msg);
	}

	#[rstest]
	//#[case::greya(DynamicImage::new_test_greya())]
	#[case::rgba(DynamicImage::new_test_rgba())]
	#[test]
	fn opaque_is_saved_without_alpha(#[case] mut img: DynamicImage) -> Result<()> {
		assert!(img.has_alpha());
		img.make_opaque()?;
		assert!(!blob2image(&encode(&img, Some(80))?)?.has_alpha());
		assert!(!blob2image(&encode(&img, Some(100))?)?.has_alpha());
		Ok(())
	}
}
