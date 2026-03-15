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
use crate::{avif, jpeg, png, webp};
use anyhow::{Result, bail};
use image::DynamicImage;
use versatiles_core::{Blob, TileFormat};
use versatiles_derive::context;

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
