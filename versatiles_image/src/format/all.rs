//! Unified image format interface for VersaTiles.
//!
//! This module abstracts over the individual format modules (`avif`, `jpeg`, `png`, `webp`) and
//! exposes two central functions — [`encode`] and [`decode`] — that dispatch to the correct codec
//! implementation based on [`TileFormat`].
//!
//! ### Supported formats
//! - **AVIF** — lossy 8‑bit encoding, optional quality/effort.
//! - **JPEG** — lossy 8‑bit RGB/L images, no alpha support.
//! - **PNG** — lossless 8‑bit L/LA/RGB/RGBA, optional effort tuning.
//! - **WebP** — lossy or lossless 8‑bit RGB/RGBA.
//!
//! Any unsupported `TileFormat` will return a `bail!` error.
use std::io::Cursor;

use anyhow::{Result, bail};
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use versatiles_core::{Blob, TileFormat};
use versatiles_derive::context;

use crate::{avif, jpeg, png, webp};

#[context("encoding {}x{} {:?} as {:?} (q={:?}, e={:?})", image.width(), image.height(), image.color(), format, quality, effort)]
/// Encode a [`DynamicImage`] into the given [`TileFormat`].
///
/// Dispatches to the corresponding codec module based on `format`.
/// Each codec interprets `quality` and `effort` slightly differently:
/// - `AVIF` uses both `quality` and `effort`.
/// - `JPG` uses only `quality`.
/// - `PNG` uses only `effort`.
/// - `WEBP` uses both `quality` and `effort`.
///
/// Returns an error if the format or color type is unsupported.
pub fn encode(image: &DynamicImage, format: TileFormat, quality: Option<u8>, effort: Option<u8>) -> Result<Blob> {
	match format {
		TileFormat::AVIF => avif::encode(image, quality, effort),
		TileFormat::JPG => jpeg::encode(image, quality),
		TileFormat::PNG => png::encode(image, effort),
		TileFormat::WEBP => webp::encode(image, quality, effort),
		_ => bail!("Unsupported format '{format}' for image encoding"),
	}
}

#[context("decoding {:?} image ({} bytes)", format, blob.len())]
/// Decode an image [`Blob`] back into a [`DynamicImage`] given its [`TileFormat`].
///
/// Dispatches to the format‑specific `blob2image()` implementation.
/// Returns an error if the format is unsupported or decoding fails.
pub fn decode(blob: &Blob, format: TileFormat) -> Result<DynamicImage> {
	match format {
		TileFormat::AVIF => avif::blob2image(blob),
		TileFormat::JPG => jpeg::blob2image(blob),
		TileFormat::PNG => png::blob2image(blob),
		TileFormat::WEBP => webp::blob2image(blob),
		_ => bail!("Unsupported format '{format}' for image decoding"),
	}
}

/// Decode `blob` as `format` with explicit limits on what the result may be.
///
/// The `image` crate's own default already caps allocation at 512 MiB, so this
/// is not the difference between bounded and unbounded — it adds the bound that
/// default does *not* carry: a cap on the dimensions themselves. A header can
/// claim 65,535 x 65,535 pixels, and refusing that outright is cheaper and
/// clearer than letting the allocation cap discover it. 16,384 is far above any
/// map tile — WebP cannot even encode past 16,383 — so nothing legitimate meets
/// it, and the allocation cap is left at the crate default so that no input
/// which decodes today stops decoding.
pub(crate) fn decode_limited(blob: &Blob, format: ImageFormat) -> Result<DynamicImage> {
	/// Longest side accepted from a decoded image, in pixels.
	const MAX_SIDE: u32 = 16_384;

	let mut limits = Limits::default();
	limits.max_image_width = Some(MAX_SIDE);
	limits.max_image_height = Some(MAX_SIDE);

	let mut reader = ImageReader::new(Cursor::new(blob.as_slice()));
	reader.set_format(format);
	reader.limits(limits);
	reader.decode().map_err(Into::into)
}

#[cfg(test)]
mod limit_tests {
	use super::*;

	/// Build a PNG that consists of nothing but a header claiming `width`x`height`.
	fn png_header(width: u32, height: u32) -> Blob {
		fn crc32(data: &[u8]) -> u32 {
			let mut crc = 0xffff_ffffu32;
			for byte in data {
				crc ^= u32::from(*byte);
				for _ in 0..8 {
					crc = if crc & 1 == 1 {
						(crc >> 1) ^ 0xedb8_8320
					} else {
						crc >> 1
					};
				}
			}
			!crc
		}

		let mut ihdr = b"IHDR".to_vec();
		ihdr.extend_from_slice(&width.to_be_bytes());
		ihdr.extend_from_slice(&height.to_be_bytes());
		ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB, no interlace

		let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
		png.extend_from_slice(&13u32.to_be_bytes());
		png.extend_from_slice(&ihdr);
		png.extend_from_slice(&crc32(&ihdr).to_be_bytes());
		Blob::from(png)
	}

	/// A header claiming more pixels than any tile could hold is refused on the
	/// strength of the dimensions alone, before the decoder works out how much
	/// memory that would take.
	#[test]
	fn oversized_dimensions_are_refused() {
		let error = decode_limited(&png_header(65_535, 65_535), ImageFormat::Png).unwrap_err();
		let message = format!("{error:#}").to_lowercase();
		assert!(message.contains("limit"), "expected a limits error, got: {error:#}");
	}

	/// A header within the limits gets past them, and fails later for the reason
	/// it should: the file carries no image data.
	#[test]
	fn ordinary_dimensions_pass_the_limits() {
		let error = decode_limited(&png_header(256, 256), ImageFormat::Png).unwrap_err();
		let message = format!("{error:#}").to_lowercase();
		assert!(
			!message.contains("limit"),
			"dimensions 256x256 must not hit a limit: {error:#}"
		);
	}
}
