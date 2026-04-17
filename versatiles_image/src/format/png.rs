//! PNG encoder/decoder utilities for `DynamicImage`.
//!
//! This module bridges the [`image`] crate’s PNG codec and the internal [`Blob`] type used by
//! VersaTiles. PNG here is treated as a **lossless** (predictor + entropy) format for tiles, with
//! an optional **effort** knob that trades compression time for file size.
//!
//! Highlights:
//! - Supports only **8‑bit** images.
//! - Accepts **L8, LA8, RGB8, RGBA8** (1–4 channels). Other layouts are rejected.
//! - If an image **has alpha but is fully opaque**, the encoder will **drop alpha** to save bytes.
//! - Uses `image::codecs::png::PngEncoder` with an effort → (compression, filter) mapping.

use crate::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};
use anyhow::{Result, anyhow, bail};
use image::{DynamicImage, ImageEncoder, ImageFormat, codecs::png, load_from_memory_with_format};
use versatiles_core::Blob;
use versatiles_derive::context;

#[context("encoding {}x{} {:?} as PNG (e={:?})", image.width(), image.height(), image.color(), effort)]
/// Encode a `DynamicImage` into a PNG [`Blob`].
///
/// * `effort` — optional 0..=100 hint (default **90**). Higher → stronger compression; lower → faster.
///   Internally mapped to `(CompressionType, FilterType)` buckets.
/// * If the image has an alpha channel but is **fully opaque**, alpha is **removed** before encoding.
/// * Errors if the image is not 8‑bit or the channel count is not in `1..=4`.
pub fn encode(image: &DynamicImage, effort: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("png only supports 8-bit images");
	}

	if image.channel_count() < 1 || image.channel_count() > 4 {
		bail!("png only supports Grey, GreyA, RGB or RGBA");
	}

	let effort = effort.unwrap_or(90).clamp(0, 100);

	use png::{CompressionType, FilterType};
	let (compression_type, filter_type) = match effort {
		0..10 => (CompressionType::Fast, FilterType::NoFilter),
		10..20 => (CompressionType::Fast, FilterType::Avg),
		20..40 => (CompressionType::Default, FilterType::Avg),
		40..60 => (CompressionType::Default, FilterType::Paeth),
		60..80 => (CompressionType::Default, FilterType::Adaptive),
		_ => (CompressionType::Best, FilterType::Adaptive),
	};

	let mut image_ref = image;
	#[allow(unused_assignments)]
	let mut optional_image: Option<DynamicImage> = None;
	if image.has_alpha() && image.is_opaque() {
		let i = image.to_no_alpha()?;
		optional_image = Some(i);
		image_ref = optional_image.as_ref().unwrap();
	}

	let mut buffer: Vec<u8> = Vec::new();
	png::PngEncoder::new_with_quality(&mut buffer, compression_type, filter_type).write_image(
		image_ref.as_bytes(),
		image_ref.width(),
		image_ref.height(),
		image_ref.extended_color_type(),
	)?;

	Ok(Blob::from(buffer))
}

#[context("encoding image {:?} as PNG", image.color())]
/// Convenience wrapper for [`encode`] with default effort.
pub fn image2blob(image: &DynamicImage) -> Result<Blob> {
	encode(image, None)
}

#[context("decoding PNG image ({} bytes)", blob.len())]
/// Decode a PNG [`Blob`] back into a [`DynamicImage`].
///
/// Returns a decoding error if the blob is not valid PNG.
pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::Png)
		.map_err(|e| anyhow!("Failed to decode PNG image: {e}"))
}

#[cfg(test)]
mod tests {
	/// PNG smoke tests: lossless round‑trip over all supported color types and
	/// verification that fully opaque images are saved **without** an alpha channel.
	use super::*;
	use crate::traits::DynamicImageTraitTest;
	use approx::assert_relative_eq;
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), 0.57)]
	#[case::greya(DynamicImage::new_test_greya(), 0.39)]
	#[case::rgb(DynamicImage::new_test_rgb(), 0.29)]
	#[case::rgba(DynamicImage::new_test_rgba(), 0.33)]
	fn png_ok(#[case] img: DynamicImage, #[case] expected_compression_percent: f64) -> Result<()> {
		let blob = image2blob(&img)?;
		let decoded = blob2image(&blob)?;

		assert_eq!(img.diff(&decoded)?, vec![0.0; img.channel_count() as usize]);

		let compression_percent = ((10_000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0;
		assert_relative_eq!(compression_percent, expected_compression_percent);

		Ok(())
	}

	#[rstest]
	#[case::greya(DynamicImage::new_test_greya())]
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
