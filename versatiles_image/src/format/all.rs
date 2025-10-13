//! Unified image format interface for VersaTiles.
//!
//! This module abstracts over the individual format modules (`avif`, `jpeg`, `png`, `webp`) and
//! exposes two central functions — [`encode`] and [`decode`] — that dispatch to the correct codec
//! implementation based on [`TileFormat`].
//!
//! ### Supported formats
//! - **AVIF** — lossy 8‑bit encoding, optional quality/speed.
//! - **JPEG** — lossy 8‑bit RGB/L images, no alpha support.
//! - **PNG** — lossless 8‑bit L/LA/RGB/RGBA, optional speed tuning.
//! - **WebP** — lossy or lossless 8‑bit RGB/RGBA.
//!
//! Any unsupported `TileFormat` will return a `bail!` error.
use crate::{avif, jpeg, png, webp};
use anyhow::{Result, bail};
use image::DynamicImage;
use versatiles_core::{Blob, TileFormat};

/// Encode a [`DynamicImage`] into the given [`TileFormat`].
///
/// Dispatches to the corresponding codec module based on `format`.
/// Each codec interprets `quality` and `speed` slightly differently:
/// - `AVIF` uses both `quality` and `speed`.
/// - `JPG` uses only `quality`.
/// - `PNG` uses only `speed`.
/// - `WEBP` uses only `quality`.
///
/// Returns an error if the format or color type is unsupported.
pub fn encode(image: &DynamicImage, format: TileFormat, quality: Option<u8>, speed: Option<u8>) -> Result<Blob> {
	match format {
		TileFormat::AVIF => avif::encode(image, quality, speed),
		TileFormat::JPG => jpeg::encode(image, quality),
		TileFormat::PNG => png::encode(image, speed),
		TileFormat::WEBP => webp::encode(image, quality),
		_ => bail!("Unsupported format '{format}' for image encoding"),
	}
}

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
