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

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageBuffer};
use libwebp_sys::{
	VP8StatusCode, WebPBitstreamFeatures, WebPConfig, WebPDecodeRGB, WebPDecodeRGBA, WebPEncode, WebPFree,
	WebPGetFeatures, WebPMemoryWrite, WebPMemoryWriter, WebPMemoryWriterClear, WebPMemoryWriterInit, WebPPicture,
	WebPPictureFree, WebPPictureImportRGB, WebPPictureImportRGBA,
};
use versatiles_core::Blob;
use versatiles_derive::context;

use crate::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};

#[context("encoding {}x{} {:?} as WebP (q={:?}, e={:?})", image.width(), image.height(), image.color(), quality, effort)]
/// Encode a `DynamicImage` into a WebP [`Blob`].
///
/// * `quality` — `Some(q)` selects **lossy** encoding for `q < 100` (0..=99), or **lossless** when
///   `q >= 100`. `None` defaults to **95** (lossy).
/// * `effort` — optional 0..=100 hint (default **80**). Higher → better compression; lower → faster.
///   Internally mapped to libwebp's `method` 0..=6 (0 = fastest, 6 = slowest/best).
/// * Only 8‑bit `Rgb8`/`Rgba8` are accepted. If the input has an alpha channel but is fully opaque,
///   alpha is removed first.
///
/// Returns an error for unsupported bit depth or color type.
pub fn encode(image: &DynamicImage, quality: Option<u8>, effort: Option<u8>) -> Result<Blob> {
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
		let i = image.to_no_alpha()?;
		optional_image = Some(i);
		image_ref = optional_image.as_ref().expect("just assigned Some above");
	}

	let quality = quality.unwrap_or(95);
	#[allow(clippy::cast_possible_wrap)]
	let width = image_ref.width() as i32;
	#[allow(clippy::cast_possible_wrap)]
	let height = image_ref.height() as i32;
	let data = image_ref.as_bytes();
	let has_alpha = image_ref.has_alpha();
	let effort = effort.unwrap_or(80);

	if quality >= 100 {
		encode_lossless(data, width, height, has_alpha, effort)
	} else {
		encode_lossy(data, width, height, has_alpha, quality, effort)
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

fn encode_lossy(data: &[u8], width: i32, height: i32, has_alpha: bool, quality: u8, effort: u8) -> Result<Blob> {
	let mut config = WebPConfig::new().map_err(|()| anyhow::anyhow!("WebPConfigInit failed"))?;
	config.lossless = 0;
	config.quality = f32::from(quality);
	// Map effort 0..=100 to libwebp method 0..=6 (effort 0 → fastest, effort 100 → best)
	#[allow(clippy::cast_possible_truncation)]
	{
		config.method = (f32::from(effort.clamp(0, 100)) / 100.0 * 6.0).round() as i32;
	}
	webp_encode_advanced(data, width, height, has_alpha, &config)
}

fn encode_lossless(data: &[u8], width: i32, height: i32, has_alpha: bool, effort: u8) -> Result<Blob> {
	let mut config = WebPConfig::new().map_err(|()| anyhow::anyhow!("WebPConfigInit failed"))?;
	config.lossless = 1;
	config.exact = 1;

	// Map effort (0=fastest, 100=best compression) to libwebp method and quality.
	let (method, quality) = if effort == 100 {
		(6, 100.0)
	} else if effort >= 90 {
		(5, 80.0)
	} else if effort >= 80 {
		(4, 70.0)
	} else if effort >= 60 {
		(2, 40.0)
	} else if effort >= 40 {
		(1, 30.0)
	} else if effort >= 20 {
		(1, 0.0)
	} else if effort >= 10 {
		(0, 50.0)
	} else {
		(0, 0.0)
	};
	config.method = method;
	config.quality = quality;

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

		let has_alpha = features.has_alpha != 0;
		let channels = if has_alpha { 4 } else { 3 };

		// `out_width`/`out_height` are written by the decoder and describe the
		// buffer it actually allocated. The header's `features.width/height`
		// describe what the file *claims*, are parsed from untrusted input, and
		// need not agree — an extended-format file carries a canvas size next to
		// its frame size. Sizing the copy from the claim rather than from the
		// buffer is a heap overread waiting for the two to diverge, so only the
		// decoder's own numbers are used below.
		let mut out_width: i32 = 0;
		let mut out_height: i32 = 0;
		let ptr = if has_alpha {
			WebPDecodeRGBA(data.as_ptr(), data.len(), &raw mut out_width, &raw mut out_height)
		} else {
			WebPDecodeRGB(data.as_ptr(), data.len(), &raw mut out_width, &raw mut out_height)
		};
		if ptr.is_null() {
			bail!(
				"Failed to decode WebP image: {} decoding failed",
				if has_alpha { "RGBA" } else { "RGB" }
			);
		}

		// The buffer is libwebp's, so it is copied and freed before any `?` can
		// return: an early exit between the two would leak it.
		let copied = copy_decoded_pixels(ptr, out_width, out_height, channels);
		WebPFree(ptr.cast::<std::ffi::c_void>());
		let (width, height, pixels) = copied?;

		if has_alpha {
			let buffer = ImageBuffer::from_raw(width, height, pixels)
				.ok_or_else(|| anyhow::anyhow!("Failed to create RGBA image buffer"))?;
			Ok(DynamicImage::ImageRgba8(buffer))
		} else {
			let buffer = ImageBuffer::from_raw(width, height, pixels)
				.ok_or_else(|| anyhow::anyhow!("Failed to create RGB image buffer"))?;
			Ok(DynamicImage::ImageRgb8(buffer))
		}
	}
}

/// Copy `width * height * channels` bytes out of a buffer libwebp allocated.
///
/// Every value that decides how much is read is validated first: the dimensions
/// come back from C as `i32` and must be positive, and their product must not
/// overflow `usize` — on a 32-bit target a large enough image would otherwise
/// wrap to a small length and, worse, a wrapped length in the other direction
/// would read past the allocation.
///
/// # Safety
/// `ptr` must point to a readable buffer of at least `width * height * channels`
/// bytes, which is what `WebPDecodeRGB`/`WebPDecodeRGBA` return alongside the
/// `width`/`height` they wrote.
unsafe fn copy_decoded_pixels(ptr: *const u8, width: i32, height: i32, channels: usize) -> Result<(u32, u32, Vec<u8>)> {
	let width = u32::try_from(width).context("WebP decoder returned a negative width")?;
	let height = u32::try_from(height).context("WebP decoder returned a negative height")?;
	ensure!(
		width > 0 && height > 0,
		"WebP decoder returned an empty image ({width}x{height})"
	);

	let size = usize::try_from(width)
		.ok()
		.and_then(|w| w.checked_mul(usize::try_from(height).ok()?))
		.and_then(|pixels| pixels.checked_mul(channels))
		.context("WebP image is too large to fit in memory on this platform")?;

	// SAFETY: the caller guarantees `ptr` covers `size` bytes; `size` is exactly
	// the buffer size implied by the dimensions the decoder reported.
	let pixels = unsafe { std::slice::from_raw_parts(ptr, size) }.to_vec();
	Ok((width, height, pixels))
}

#[cfg(test)]
mod tests {
	use approx::assert_relative_eq;
	use rstest::rstest;

	/// WebP tests: lossy & lossless success cases, rejection of grey/greya inputs,
	/// and verification that fully opaque RGBA is stored without alpha.
	use super::*;
	use crate::traits::DynamicImageTraitTest;

	/// The decoded image is sized by the decoder, not by the file header, and a
	/// roundtrip still returns the dimensions it went in with.
	#[test]
	fn decode_uses_the_decoder_dimensions() -> Result<()> {
		for image in [DynamicImage::new_test_rgb(), DynamicImage::new_test_rgba()] {
			let decoded = blob2image(&image2blob_lossless(&image)?)?;
			assert_eq!(
				(decoded.width(), decoded.height()),
				(image.width(), image.height()),
				"roundtrip changed the image size"
			);
		}
		Ok(())
	}

	/// Input the decoder rejects must produce an error rather than a copy out of
	/// a buffer that was never allocated. The truncated case matters most: the
	/// header still announces the full size, so a copy sized from the header
	/// would read past what libwebp returned.
	#[test]
	fn decode_rejects_malformed_input() -> Result<()> {
		assert!(blob2image(&Blob::from(Vec::new())).is_err(), "empty input");
		assert!(
			blob2image(&Blob::from(b"RIFF____WEBPVP8 ".to_vec())).is_err(),
			"header only"
		);

		let blob = image2blob_lossless(&DynamicImage::new_test_rgb())?;
		let bytes = blob.as_slice();
		let truncated = Blob::from(bytes[..bytes.len() / 2].to_vec());
		assert!(blob2image(&truncated).is_err(), "truncated input");

		Ok(())
	}

	#[rstest]
	#[case::rgb(          DynamicImage::new_test_rgb(),  false, 0.95, vec![0.9, 0.5, 1.5]     )]
	#[case::rgba(         DynamicImage::new_test_rgba(), false, 0.75, vec![0.9, 0.5, 1.5, 0.0])]
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

		assert_relative_eq!(
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
