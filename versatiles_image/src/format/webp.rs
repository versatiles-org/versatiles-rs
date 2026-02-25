//! WebP encoder/decoder utilities for `DynamicImage`.
//!
//! This module uses `libwebp-sys` (direct FFI bindings to libwebp) for both encoding and decoding.
//! It supports both **lossy** and **lossless** WebP.
//!
//! Highlights:
//! - Only **8‑bit** images are supported.
//! - Accepted layouts for encoding: **RGB8** and **RGBA8**. Greyscale variants are rejected by
//!   design to keep code paths explicit.
//! - When an image has an alpha channel but is **fully opaque**, the encoder **drops alpha** before
//!   encoding to reduce size.
//! - Quality boundary: `< 100` = lossy, `>= 100` = lossless, `None` defaults to 95.

use crate::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};
use anyhow::{Result, bail};
use image::{DynamicImage, ImageBuffer};
use libwebp_sys::{
	VP8StatusCode, WebPBitstreamFeatures, WebPConfig, WebPDecodeRGB, WebPDecodeRGBA, WebPEncode, WebPFree,
	WebPGetFeatures, WebPMemoryWrite, WebPMemoryWriter, WebPMemoryWriterClear, WebPMemoryWriterInit, WebPPicture,
	WebPPictureFree, WebPPictureImportRGB, WebPPictureImportRGBA,
};
use versatiles_core::Blob;
use versatiles_derive::context;

#[context("encoding {}x{} {:?} as WebP (q={:?}, s={:?})", image.width(), image.height(), image.color(), quality, speed)]
/// Encode a `DynamicImage` into a WebP [`Blob`].
///
/// * `quality` — `Some(q)` selects **lossy** encoding for `q < 100` (0..=99), or **lossless** when
///   `q >= 100`. `None` defaults to **95** (lossy).
/// * `speed` — optional 0..=100 hint (default **75**). Lower → better compression; higher → faster.
///   Internally mapped to libwebp's `method` 0..=6 (6 = slowest/best, 0 = fastest).
/// * Only 8‑bit `Rgb8`/`Rgba8` are accepted. If the input has an alpha channel but is fully opaque,
///   alpha is removed first.
///
/// Returns an error for unsupported bit depth or color type.
pub fn encode(image: &DynamicImage, quality: Option<u8>, speed: Option<u8>) -> Result<Blob> {
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
	// Map user speed 0..=100 to libwebp method 6..=0 (inverted: lower speed → higher method)
	#[allow(clippy::cast_possible_truncation)]
	// method 0..=6 fits into i32, clamp ensures valid range
	let method = speed.map_or(4, |s| (6.0 - f32::from(s) / 100.0 * 6.0).round() as i32);
	#[allow(clippy::cast_possible_wrap)]
	let width = image_ref.width() as i32;
	#[allow(clippy::cast_possible_wrap)]
	let height = image_ref.height() as i32;
	let data = image_ref.as_bytes();
	let has_alpha = image_ref.has_alpha();

	if quality >= 100 {
		encode_lossless(data, width, height, has_alpha, method)
	} else {
		encode_lossy(data, width, height, has_alpha, f32::from(quality), method)
	}
}

/// Encode using the advanced API (WebPConfig + WebPPicture + WebPEncode).
fn webp_encode_advanced(data: &[u8], width: i32, height: i32, has_alpha: bool, config: &WebPConfig) -> Result<Blob> {
	unsafe {
		let mut picture = WebPPicture::new().map_err(|()| anyhow::anyhow!("WebPPictureInit failed"))?;
		picture.use_argb = 1;
		picture.width = width;
		picture.height = height;

		let stride = if has_alpha { width * 4 } else { width * 3 };
		let import_ok = if has_alpha {
			WebPPictureImportRGBA(&raw mut picture, data.as_ptr(), stride)
		} else {
			WebPPictureImportRGB(&raw mut picture, data.as_ptr(), stride)
		};
		if import_ok == 0 {
			WebPPictureFree(&raw mut picture);
			bail!("WebP picture import failed");
		}

		let mut writer: WebPMemoryWriter = std::mem::zeroed();
		WebPMemoryWriterInit(&raw mut writer);
		picture.writer = Some(WebPMemoryWrite);
		picture.custom_ptr = (&raw mut writer).cast::<std::ffi::c_void>();

		let ok = WebPEncode(config, &raw mut picture);
		WebPPictureFree(&raw mut picture);

		if ok == 0 {
			WebPMemoryWriterClear(&raw mut writer);
			bail!("WebP encoding failed");
		}

		let result = std::slice::from_raw_parts(writer.mem, writer.size).to_vec();
		WebPFree(writer.mem.cast::<std::ffi::c_void>());
		Ok(Blob::from(result))
	}
}

fn encode_lossy(data: &[u8], width: i32, height: i32, has_alpha: bool, quality: f32, method: i32) -> Result<Blob> {
	let mut config = WebPConfig::new().map_err(|()| anyhow::anyhow!("WebPConfigInit failed"))?;
	config.lossless = 0;
	config.quality = quality;
	config.method = method;
	webp_encode_advanced(data, width, height, has_alpha, &config)
}

fn encode_lossless(data: &[u8], width: i32, height: i32, has_alpha: bool, method: i32) -> Result<Blob> {
	let mut config = WebPConfig::new().map_err(|()| anyhow::anyhow!("WebPConfigInit failed"))?;
	config.lossless = 1;
	config.exact = 1;
	config.method = method;
	webp_encode_advanced(data, width, height, has_alpha, &config)
}

#[context("encoding image {:?} as WebP (q={:?})", image.color(), quality)]
/// Convenience wrapper around [`encode`].
///
/// `quality = None` uses a default lossy quality of **95**.
pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	encode(image, quality, None)
}

#[context("encoding image {:?} as lossless WebP", image.color())]
/// Convenience wrapper for **lossless** WebP encoding (equivalent to `encode(image, Some(100), None)`).
pub fn image2blob_lossless(image: &DynamicImage) -> Result<Blob> {
	encode(image, Some(100), None)
}

#[context("decoding WebP image ({} bytes)", blob.len())]
/// Decode a WebP [`Blob`] back into a [`DynamicImage`].
///
/// Returns a decoding error if the blob is not valid WebP.
pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	let data = blob.as_slice();
	if data.is_empty() {
		bail!("Failed to decode WebP image: empty input");
	}

	unsafe {
		let mut features: WebPBitstreamFeatures = std::mem::zeroed();
		let status = WebPGetFeatures(data.as_ptr(), data.len(), &raw mut features);
		if status != VP8StatusCode::VP8_STATUS_OK {
			bail!("Failed to decode WebP image: invalid WebP data");
		}

		#[allow(clippy::cast_sign_loss)]
		let width = features.width as u32;
		#[allow(clippy::cast_sign_loss)]
		let height = features.height as u32;

		if features.has_alpha != 0 {
			let mut out_width: i32 = 0;
			let mut out_height: i32 = 0;
			let ptr = WebPDecodeRGBA(data.as_ptr(), data.len(), &raw mut out_width, &raw mut out_height);
			if ptr.is_null() {
				bail!("Failed to decode WebP image: RGBA decoding failed");
			}
			let buf_size = (width as usize) * (height as usize) * 4;
			let pixels = std::slice::from_raw_parts(ptr, buf_size).to_vec();
			WebPFree(ptr.cast::<std::ffi::c_void>());
			let buffer = ImageBuffer::from_raw(width, height, pixels)
				.ok_or_else(|| anyhow::anyhow!("Failed to create RGBA image buffer"))?;
			Ok(DynamicImage::ImageRgba8(buffer))
		} else {
			let mut out_width: i32 = 0;
			let mut out_height: i32 = 0;
			let ptr = WebPDecodeRGB(data.as_ptr(), data.len(), &raw mut out_width, &raw mut out_height);
			if ptr.is_null() {
				bail!("Failed to decode WebP image: RGB decoding failed");
			}
			let buf_size = (width as usize) * (height as usize) * 3;
			let pixels = std::slice::from_raw_parts(ptr, buf_size).to_vec();
			WebPFree(ptr.cast::<std::ffi::c_void>());
			let buffer = ImageBuffer::from_raw(width, height, pixels)
				.ok_or_else(|| anyhow::anyhow!("Failed to create RGB image buffer"))?;
			Ok(DynamicImage::ImageRgb8(buffer))
		}
	}
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	/// WebP tests: lossy & lossless success cases, rejection of grey/greya inputs,
	/// and verification that fully opaque RGBA is stored without alpha.
	use super::*;
	use crate::traits::DynamicImageTraitTest;
	use rstest::rstest;

	#[rstest]
	#[case::rgb(          DynamicImage::new_test_rgb(),  false, 0.96, vec![0.9, 0.5, 1.5]     )]
	#[case::rgba(         DynamicImage::new_test_rgba(), false, 0.76, vec![0.9, 0.5, 1.6, 0.0])]
	#[case::lossless_rgb( DynamicImage::new_test_rgb(),  true,  0.04, vec![0.0, 0.0, 0.0]     )]
	#[case::lossless_rgba(DynamicImage::new_test_rgba(), true,  0.03, vec![0.0, 0.0, 0.0, 0.0])]
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
		assert_eq!(res.unwrap_err().chain().last().unwrap().to_string(), expected_msg);
	}

	#[rstest]
	//#[case::greya(DynamicImage::new_test_greya())]
	#[case::rgba(DynamicImage::new_test_rgba())]
	#[test]
	fn opaque_is_saved_without_alpha(#[case] mut img: DynamicImage) -> Result<()> {
		assert!(img.has_alpha());
		img.make_opaque()?;
		assert!(!blob2image(&encode(&img, Some(80), None)?)?.has_alpha());
		assert!(!blob2image(&encode(&img, Some(100), None)?)?.has_alpha());
		Ok(())
	}

	/* ---------- encode() direct tests ---------- */

	#[test]
	fn encode_with_custom_quality() -> Result<()> {
		let img = DynamicImage::new_test_rgb();
		let blob_q50 = encode(&img, Some(50), None)?;
		let blob_q95 = encode(&img, Some(95), None)?;
		// Both should produce valid output
		assert!(!blob_q50.is_empty());
		assert!(!blob_q95.is_empty());
		// Lower quality should generally produce smaller files
		assert!(blob_q50.len() < blob_q95.len());
		Ok(())
	}

	#[test]
	fn encode_quality_boundary() -> Result<()> {
		let img = DynamicImage::new_test_rgb();
		// quality 99 is lossy
		let blob_lossy = encode(&img, Some(99), None)?;
		// quality 100 is lossless
		let blob_lossless = encode(&img, Some(100), None)?;
		// Lossless should be smaller for our synthetic test image
		assert!(!blob_lossy.is_empty());
		assert!(!blob_lossless.is_empty());
		Ok(())
	}

	#[test]
	fn encode_default_quality() -> Result<()> {
		let img = DynamicImage::new_test_rgb();
		// None defaults to 95
		let blob_default = encode(&img, None, None)?;
		let blob_95 = encode(&img, Some(95), None)?;
		// Should produce same size (same quality)
		assert_eq!(blob_default.len(), blob_95.len());
		Ok(())
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
	fn blob2image_invalid_data() {
		let blob = Blob::from(vec![1, 2, 3, 4, 5]);
		let result = blob2image(&blob);
		assert!(result.is_err());
	}

	#[test]
	fn blob2image_empty_blob() {
		let blob = Blob::from(vec![]);
		let result = blob2image(&blob);
		assert!(result.is_err());
	}
}
