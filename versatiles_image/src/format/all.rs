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
/// is not the difference between bounded and unbounded. It adds the two bounds
/// that default does *not* carry: a cap on the dimensions themselves, and a cap
/// on the decoded byte size that this crate checks rather than delegating.
///
/// The dimension cap is the cheap one — a header can claim 65,535 x 65,535, and
/// refusing that outright is clearer than letting an allocation cap discover it.
/// The byte cap exists because the two are not the same question: 16,384 x
/// 16,384 passes a 16,384-per-side cap and is still a gigabyte. It is checked
/// here, against the header, because `Limits` binds only decoders that consult
/// it and `zune-jpeg` does not read `max_alloc`.
pub(crate) fn decode_limited(blob: &Blob, format: ImageFormat) -> Result<DynamicImage> {
	// Read the header first and judge it ourselves. `Limits` is only as good as
	// the decoder that consults it — `zune-jpeg`, behind `ImageFormat::Jpeg`,
	// does not read `max_alloc` — so a dimension pair that passes the width and
	// height caps but multiplies out to hundreds of megabytes would still be
	// decoded. `into_dimensions` parses the header without decoding, which makes
	// the check independent of any decoder's cooperation.
	let mut probe = ImageReader::new(Cursor::new(blob.as_slice()));
	probe.set_format(format);
	let (width, height) = probe.into_dimensions()?;
	ensure_decodable(width, height, MAX_BYTES_PER_PIXEL)?;

	let mut limits = Limits::default();
	limits.max_image_width = Some(MAX_SIDE);
	limits.max_image_height = Some(MAX_SIDE);

	let mut reader = ImageReader::new(Cursor::new(blob.as_slice()));
	reader.set_format(format);
	reader.limits(limits);
	reader.decode().map_err(Into::into)
}

/// Longest side accepted from a decoded image, in pixels.
///
/// Far above any map tile — WebP cannot even encode past 16,383 — so nothing
/// legitimate meets it.
pub(crate) const MAX_SIDE: u32 = 16_384;

/// Widest pixel this crate decodes to: RGBA, four bytes.
pub(crate) const MAX_BYTES_PER_PIXEL: u64 = 4;

/// Ceiling on the decoded size of one image.
///
/// The same 512 MiB the `image` crate applies by default, restated as a number
/// this crate enforces itself so that formats whose decoders ignore `Limits`
/// — WebP goes through libwebp directly, JPEG through `zune-jpeg` — are held to
/// it too. Keeping it equal to the crate default means nothing that decodes
/// today stops decoding.
pub(crate) const MAX_DECODED_BYTES: u64 = 512 * 1024 * 1024;

/// Refuse dimensions whose decoded form would be larger than we accept, before
/// anything allocates for them.
///
/// # Errors
/// Returns an error if either side exceeds [`MAX_SIDE`], or if
/// `width * height * bytes_per_pixel` exceeds [`MAX_DECODED_BYTES`].
pub(crate) fn ensure_decodable(width: u32, height: u32, bytes_per_pixel: u64) -> Result<()> {
	anyhow::ensure!(
		width <= MAX_SIDE && height <= MAX_SIDE,
		"image is {width}x{height}, which exceeds the {MAX_SIDE} pixel limit per side"
	);

	// Cannot overflow: both sides are bounded above by MAX_SIDE (2^14) and the
	// per-pixel count by 4, so the product fits in 32 bits with room to spare.
	let bytes = u64::from(width) * u64::from(height) * bytes_per_pixel;
	anyhow::ensure!(
		bytes <= MAX_DECODED_BYTES,
		"image is {width}x{height}, which decodes to {bytes} bytes and exceeds the {MAX_DECODED_BYTES} byte limit"
	);

	Ok(())
}

#[cfg(test)]
mod limit_tests {
	use super::*;

	/// Build a structurally complete PNG that carries no pixels, claiming `width`x`height`.
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

		// An empty IDAT and an IEND, so the header parses to completion. Reading
		// the dimensions needs the chunk after IHDR to exist; with IHDR alone the
		// decoder reaches end-of-file first and reports that instead of anything
		// about the dimensions it was asked for.
		for tag in [b"IDAT", b"IEND"] {
			png.extend_from_slice(&0u32.to_be_bytes());
			png.extend_from_slice(tag);
			png.extend_from_slice(&crc32(tag).to_be_bytes());
		}

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

	/// Both sides can sit under the per-side cap while their product does not:
	/// 16384x16384 is within 16384 on each axis and still a gigabyte decoded.
	/// The per-side cap alone does not catch that, which is what the byte
	/// ceiling is for — and why it is checked here rather than left to a
	/// decoder that may not consult `Limits` at all.
	#[test]
	fn dimensions_within_the_side_cap_can_still_be_too_large() {
		let error = decode_limited(&png_header(MAX_SIDE, MAX_SIDE), ImageFormat::Png).unwrap_err();
		let message = format!("{error:#}");
		assert!(
			message.contains("byte limit"),
			"expected the byte ceiling to refuse it, got: {error:#}"
		);
	}

	#[test]
	fn ensure_decodable_accepts_what_it_should() {
		// An ordinary tile.
		assert!(ensure_decodable(256, 256, 4).is_ok());
		// Exactly on the per-side cap, small enough in bytes at one byte each.
		assert!(ensure_decodable(MAX_SIDE, 1, 1).is_ok());
		// Exactly on the byte ceiling: 16384 * 8192 * 4 == 512 MiB.
		assert!(ensure_decodable(16_384, 8_192, 4).is_ok());
	}

	#[test]
	fn ensure_decodable_refuses_what_it_should() {
		// One past the per-side cap.
		assert!(ensure_decodable(MAX_SIDE + 1, 1, 1).is_err());
		assert!(ensure_decodable(1, MAX_SIDE + 1, 1).is_err());
		// One row past the byte ceiling.
		assert!(ensure_decodable(16_384, 8_193, 4).is_err());
	}
}
