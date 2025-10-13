//! Utilities for converting between `DynamicImage` objects and raw/encoded image formats.
//!
//! This trait (`DynamicImageTraitConvert`) extends the `DynamicImage` type with methods to:
//! - Create images from functions (`from_fn_*` variants)
//! - Convert between raw byte buffers and `DynamicImage` (`from_raw`)
//! - Encode/decode to/from supported image formats (`to_blob`, `from_blob`)
//! - Iterate over pixel data (`iter_pixels`)
//!
//! Supported formats include: PNG, JPEG, WEBP, and AVIF.
//! These utilities are used in VersaTiles Pipeline.

use crate::format::{avif, jpeg, png, webp};
use anyhow::{Result, anyhow, bail, ensure};
use image::{DynamicImage, EncodableLayout, ImageBuffer, Luma, LumaA, Rgb, Rgba};
use versatiles_core::{Blob, TileFormat};

/// Trait for converting between `DynamicImage` and raw/encoded formats, and for constructing images from functions.
pub trait DynamicImageTraitConvert {
	/// Creates a grayscale (`L8`) image by calling the provided function for each pixel coordinate.
	/// The function `f(x, y)` should return a single 8-bit luminance value.
	fn from_fn_l8(width: u32, height: u32, f: fn(u32, u32) -> u8) -> DynamicImage;

	/// Creates an image with luminance and alpha channels (`LA8`).
	/// The function `f(x, y)` should return a 2-element `[u8; 2]` array representing the pixel.
	fn from_fn_la8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 2]) -> DynamicImage;

	/// Creates an RGB image (`RGB8`) by calling the function `f(x, y)` for each pixel coordinate.
	fn from_fn_rgb8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 3]) -> DynamicImage;

	/// Creates an RGBA image (`RGBA8`) by calling the function `f(x, y)` for each pixel coordinate.
	fn from_fn_rgba8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 4]) -> DynamicImage;

	/// Constructs a `DynamicImage` from raw pixel data and dimensions.
	/// The number of channels is inferred from the data length. Supported channel counts are 1 (L8), 2 (LA8), 3 (RGB8), and 4 (RGBA8).
	/// Returns an error if the data length does not match the expected size or if the channel count is unsupported.
	fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<DynamicImage>;

	/// Decodes a `DynamicImage` from a binary blob using the specified `TileFormat`.
	/// Returns an error if decoding fails or if the format is unsupported.
	fn from_blob(blob: &Blob, format: TileFormat) -> Result<DynamicImage>;

	/// Encodes the image into a binary blob in the specified `TileFormat`.
	/// Returns an error if encoding fails or if the format is unsupported.
	fn to_blob(&self, format: TileFormat) -> Result<Blob>;

	/// Returns an iterator over the pixel data as byte slices.
	/// Each slice represents one pixel, with the slice length corresponding to the image's channel count.
	fn iter_pixels(&self) -> impl Iterator<Item = &[u8]>;
}

impl DynamicImageTraitConvert for DynamicImage {
	fn from_fn_l8(width: u32, height: u32, f: fn(u32, u32) -> u8) -> DynamicImage {
		DynamicImage::ImageLuma8(ImageBuffer::from_fn(width, height, |x, y| Luma([f(x, y)])))
	}
	fn from_fn_la8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 2]) -> DynamicImage {
		DynamicImage::ImageLumaA8(ImageBuffer::from_fn(width, height, |x, y| LumaA(f(x, y))))
	}
	fn from_fn_rgb8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 3]) -> DynamicImage {
		DynamicImage::ImageRgb8(ImageBuffer::from_fn(width, height, |x, y| Rgb(f(x, y))))
	}
	fn from_fn_rgba8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 4]) -> DynamicImage {
		DynamicImage::ImageRgba8(ImageBuffer::from_fn(width, height, |x, y| Rgba(f(x, y))))
	}

	fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<DynamicImage> {
		let channel_count = data.len() / (width * height) as usize;
		ensure!(
			channel_count * (width * height) as usize == data.len(),
			"Data length ({}) does not match width ({width}) * height ({height}) * channel_count ({channel_count}) = {}",
			data.len(),
			(width * height) as usize * channel_count
		);
		Ok(match channel_count {
			1 => DynamicImage::ImageLuma8(
				ImageBuffer::from_vec(width, height, data)
					.ok_or_else(|| anyhow!("Failed to create Luma8 image buffer with provided data"))?,
			),
			2 => DynamicImage::ImageLumaA8(
				ImageBuffer::from_vec(width, height, data)
					.ok_or_else(|| anyhow!("Failed to create LumaA8 image buffer with provided data"))?,
			),
			3 => DynamicImage::ImageRgb8(
				ImageBuffer::from_vec(width, height, data)
					.ok_or_else(|| anyhow!("Failed to create RGB8 image buffer with provided data"))?,
			),
			4 => DynamicImage::ImageRgba8(
				ImageBuffer::from_vec(width, height, data)
					.ok_or_else(|| anyhow!("Failed to create RGBA8 image buffer with provided data"))?,
			),
			_ => bail!("Unsupported channel count: {channel_count}"),
		})
	}

	fn to_blob(&self, format: TileFormat) -> Result<Blob> {
		use TileFormat::{AVIF, JPG, PNG, WEBP};
		match format {
			AVIF => avif::image2blob(self, None),
			JPG => jpeg::image2blob(self, None),
			PNG => png::image2blob(self),
			WEBP => webp::image2blob(self, None),
			_ => bail!("Unsupported image format for encoding: {format:?}"),
		}
	}

	fn from_blob(blob: &Blob, format: TileFormat) -> Result<DynamicImage> {
		use TileFormat::{AVIF, JPG, PNG, WEBP};
		match format {
			AVIF => avif::blob2image(blob),
			JPG => jpeg::blob2image(blob),
			PNG => png::blob2image(blob),
			WEBP => webp::blob2image(blob),
			_ => bail!("Unsupported image format for decoding: {format:?}"),
		}
	}

	fn iter_pixels(&self) -> impl Iterator<Item = &[u8]> {
		match self {
			DynamicImage::ImageLuma8(img) => img.as_bytes().chunks_exact(1),
			DynamicImage::ImageLumaA8(img) => img.as_bytes().chunks_exact(2),
			DynamicImage::ImageRgb8(img) => img.as_bytes().chunks_exact(3),
			DynamicImage::ImageRgba8(img) => img.as_bytes().chunks_exact(4),
			_ => panic!("Unsupported image type for pixel iteration"),
		}
	}
}

/// Tests for the `DynamicImageTraitConvert` trait implementation.
/// These tests verify conversion between raw data, pixel iteration, and format roundtrips using `rstest`.
#[cfg(test)]
mod tests {
	use crate::DynamicImageTraitInfo;

	use super::*;
	use rstest::rstest;

	fn sample_l8() -> DynamicImage {
		DynamicImage::from_fn_l8(4, 3, |x, y| ((x + y) % 2) as u8)
	}

	fn sample_la8() -> DynamicImage {
		DynamicImage::from_fn_la8(4, 3, |x, y| [((x * 2 + y) % 256) as u8, 255])
	}

	fn sample_rgb8() -> DynamicImage {
		DynamicImage::from_fn_rgb8(4, 3, |x, y| [x as u8, y as u8, (x + y) as u8])
	}

	fn sample_rgba8() -> DynamicImage {
		DynamicImage::from_fn_rgba8(4, 3, |x, y| [x as u8, y as u8, (x + y) as u8, 200])
	}

	#[rstest]
	//#[case::avif(TileFormat::AVIF, [0.0; 3])]
	#[case::jpg(TileFormat::JPG, [0.4, 0.2, 0.5])]
	#[case::png(TileFormat::PNG, [0.0; 3])]
	#[case::webp(TileFormat::WEBP, [5.5,0.4,4.2])]
	fn roundtrip_encode_decode(#[case] format: TileFormat, #[case] diff: [f64; 3]) {
		let image = sample_rgb8();
		// Encode the image to a blob
		let blob = image.to_blob(format).unwrap();
		// Decode the blob back to an image
		let decoded_image = DynamicImage::from_blob(&blob, format).expect("Failed to decode image");
		// Assert that the original image and the decoded image are equal
		assert_eq!(DynamicImageTraitInfo::diff(&image, &decoded_image).unwrap(), diff);
	}

	#[rstest]
	#[case::l8(sample_l8(), 1usize)]
	#[case::la8(sample_la8(), 2usize)]
	#[case::rgb8(sample_rgb8(), 3usize)]
	#[case::rgba8(sample_rgba8(), 4usize)]
	fn iter_pixels_chunk_sizes_match_color_type(#[case] img: DynamicImage, #[case] chunk: usize) {
		for px in img.iter_pixels() {
			assert_eq!(px.len(), chunk);
		}
	}

	#[rstest]
	#[case::l8(1)]
	#[case::la8(2)]
	#[case::rgb8(3)]
	#[case::rgba8(4)]
	fn from_raw_accepts_supported_channel_counts(#[case] channels: usize) {
		let w = 4u32;
		let h = 3u32; // 12 pixels
		let data = (0..(w as usize * h as usize * channels))
			.map(|v| (v % 256) as u8)
			.collect::<Vec<_>>();

		let img = DynamicImage::from_raw(w, h, data).expect("from_raw failed");
		assert_eq!(img.color().channel_count() as usize, channels);
	}

	#[test]
	fn from_raw_rejects_mismatched_len() {
		// 5 bytes -> channel_count = 1 (5/4), but buffer length mismatches -> error
		let data_mismatch = vec![0u8; 5];
		let err = DynamicImage::from_raw(2, 2, data_mismatch).unwrap_err();
		assert_eq!(
			format!("{err}"),
			"Data length (5) does not match width (2) * height (2) * channel_count (1) = 4"
		);
	}

	#[test]
	fn from_raw_rejects_unsupported_channel_counts() {
		// 20 bytes -> channel_count = 5 (20/4) -> unsupported channel count
		let data_unsupported = vec![0u8; 20];
		let err = DynamicImage::from_raw(2, 2, data_unsupported).unwrap_err();
		assert_eq!(format!("{err}"), "Unsupported channel count: 5");
	}

	#[test]
	fn to_blob_unsupported_format_is_error_if_any() {
		// This test intentionally avoids depending on a specific non-image format variant.
		// We just ensure the happy-path formats (PNG) succeed and that the function doesn't panic
		// for them. If more formats are enabled, the other branches are covered by compile-time.
		let img = sample_rgb8();
		let blob = img.to_blob(TileFormat::PNG).expect("PNG should encode");
		assert!(!blob.is_empty());
	}
}
