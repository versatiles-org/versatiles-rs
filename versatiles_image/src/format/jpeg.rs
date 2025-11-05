//! JPEG (Joint Photographic Experts Group) encoder and decoder utilities for `DynamicImage`.
//!
//! This module implements encoding and decoding bridges between the [`image`] crate and the
//! internal [`Blob`] type used by VersaTiles. It intentionally supports **only 8‑bit Grey and
//! RGB images**—alpha channels are not allowed since the JPEG format does not include transparency.
//!
//! Key characteristics:
//! - Supports configurable lossy quality via [`encode`].
//! - Rejects `quality >= 100` (JPEG cannot produce true lossless output).
//! - Rejects any input with an alpha channel (`LA8`, `RGBA8`, etc.).
//! - Uses the standard `image::codecs::jpeg::JpegEncoder` backend for compression.
//!
//! Example usage:
//!
//! ```no_run
//! use versatiles_image::format::jpeg::{encode, blob2image};
//! use image::DynamicImage;
//!
//! let img: DynamicImage = image::DynamicImage::new_rgb8(256, 256);
//! let blob = encode(&img, Some(90)).expect("encode ok");
//! let decoded = blob2image(&blob).expect("decode ok");
//! ```
use crate::traits::DynamicImageTraitInfo;
use anyhow::{Result, anyhow, bail};
use image::{DynamicImage, ImageEncoder, ImageFormat, codecs::jpeg::JpegEncoder, load_from_memory_with_format};
use versatiles_core::Blob;
use versatiles_derive::context;

/// Encode a `DynamicImage` into a JPEG [`Blob`].
///
/// * `quality` — 0..=99; higher means better visual quality but larger output. Defaults to **95**.
/// * Returns an error if the image is not 8‑bit, has an alpha channel, or if `quality >= 100`.
///
/// Supported color types: **L8 (Grey)** and **Rgb8**.
#[context("encoding {}x{} {:?} as JPEG (q={:?})", image.width(), image.height(), image.color(), quality)]
pub fn encode(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("JPEG only supports 8-bit images");
	}

	let quality = quality.unwrap_or(95);
	if quality >= 100 {
		bail!("JPEG does not support lossless compression, use a quality < 100");
	}

	match image.channel_count() {
		1 | 3 => image,
		_ => bail!("JPEG only supports Grey or RGB images without alpha channel"),
	};

	let mut buffer: Vec<u8> = Vec::new();
	JpegEncoder::new_with_quality(&mut buffer, quality).write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.extended_color_type(),
	)?;

	Ok(Blob::from(buffer))
}

/// Convenience wrapper around [`encode`].
///
/// Equivalent to calling `encode(image, quality)`.
#[context("encoding image {:?} as JPEG (q={:?})", image.color(), quality)]
pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	encode(image, quality)
}

/// Decode a JPEG [`Blob`] back into a [`DynamicImage`].
///
/// Returns a decoding error if the blob is not a valid JPEG.
#[context("decoding JPEG image ({} bytes)", blob.len())]
pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::Jpeg)
		.map_err(|e| anyhow!("Failed to decode JPEG image: {e}"))
}

/// Tests for JPEG encoding and decoding.
/// Ensures correct behavior for both supported and unsupported (alpha) image types.
#[cfg(test)]
mod tests {
	use super::*;
	use crate::traits::DynamicImageTraitTest;
	use rstest::rstest;

	/* ---------- Success cases (no alpha) ---------- */
	#[rstest]
	#[case::grey( DynamicImage::new_test_grey(),  6.61, vec![0.0]           )]
	#[case::rgb(  DynamicImage::new_test_rgb(),   4.65, vec![0.6, 0.3, 0.7] )]
	fn jpeg_ok(
		#[case] img: DynamicImage,
		#[case] expected_compression_percent: f64,
		#[case] expected_diff: Vec<f64>,
	) -> Result<()> {
		let blob = image2blob(&img, None)?;
		let decoded = blob2image(&blob)?;
		assert_eq!(img.diff(&decoded)?, expected_diff);

		assert_eq!(
			((10000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0,
			expected_compression_percent
		);
		Ok(())
	}

	/* ---------- Error cases (alpha not supported) ---------- */
	#[rstest]
	#[case::greya(DynamicImage::new_test_greya())]
	#[case::rgba(DynamicImage::new_test_rgba())]
	fn jpeg_rejects_alpha_images(#[case] img: DynamicImage) {
		assert_eq!(
			image2blob(&img, None).unwrap_err().chain().last().unwrap().to_string(),
			"JPEG only supports Grey or RGB images without alpha channel"
		);
	}
}
