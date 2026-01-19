//! Transparency detection for raster tiles.
//!
//! This module provides efficient methods to check if a tile is empty (fully transparent)
//! or opaque (no transparency). It uses fast paths when possible:
//! - JPEG: Always opaque, never empty (no alpha channel support)
//! - PNG/WebP: Parse header to check for alpha channel, only decode if necessary

use super::Tile;
use anyhow::Result;
use std::borrow::Cow;
use versatiles_core::{Blob, TileCompression, TileFormat, compression::decompress_ref};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitInfo;

impl Tile {
	/// Returns `true` if the tile image is fully transparent (all alpha = 0).
	///
	/// For images without an alpha channel, always returns `false`.
	/// Uses fast paths when possible:
	/// - JPEG: Always returns `false` (no alpha channel)
	/// - PNG/WebP: Parses header to check for alpha, only decodes if necessary
	#[must_use = "this returns the empty status, it doesn't modify anything"]
	#[context("checking if tile is empty (format={:?})", self.format)]
	pub fn is_empty(&mut self) -> Result<bool> {
		Ok(self.compute_transparency()?.0)
	}

	/// Returns `true` if the tile image is fully opaque (all alpha = 255 or no alpha).
	///
	/// For images without an alpha channel, always returns `true`.
	/// Uses fast paths when possible:
	/// - JPEG: Always returns `true` (no alpha channel)
	/// - PNG/WebP: Parses header to check for alpha, only decodes if necessary
	#[must_use = "this returns the opaque status, it doesn't modify anything"]
	#[context("checking if tile is opaque (format={:?})", self.format)]
	pub fn is_opaque(&mut self) -> Result<bool> {
		Ok(self.compute_transparency()?.1)
	}

	/// Internal method to compute and cache transparency info.
	/// Returns (is_empty, is_opaque).
	fn compute_transparency(&mut self) -> Result<(bool, bool)> {
		// Check cache first
		if let Some(cached) = self.transparency_cache {
			return Ok(cached);
		}

		let result = self.compute_transparency_uncached()?;
		self.transparency_cache = Some(result);
		Ok(result)
	}

	/// Compute transparency without caching.
	fn compute_transparency_uncached(&mut self) -> Result<(bool, bool)> {
		// Fast path: JPEG never has alpha
		if self.format == TileFormat::JPG {
			return Ok((false, true));
		}

		// If content is already materialized, use pixel-scanning methods
		if self.content.is_some() {
			let image = self.as_image()?;
			return Ok((image.is_empty(), image.is_opaque()));
		}

		// Try header-based detection for blob
		if self.blob.is_some() {
			let alpha_info = self.check_alpha_from_header()?;

			match alpha_info {
				AlphaInfo::NoAlpha => {
					// No alpha channel means not empty and fully opaque
					return Ok((false, true));
				}
				AlphaInfo::HasAlpha | AlphaInfo::Unknown => {
					// Must decode and scan pixels
				}
			}
		}

		// Fallback: full decode + pixel scan
		let image = self.as_image()?;
		Ok((image.is_empty(), image.is_opaque()))
	}

	/// Check alpha channel presence from image header without full decode.
	fn check_alpha_from_header(&self) -> Result<AlphaInfo> {
		let Some(blob) = &self.blob else {
			return Ok(AlphaInfo::Unknown);
		};

		// Decompress if needed (temporary, doesn't modify tile state)
		let data: Cow<'_, Blob> = if self.compression == TileCompression::Uncompressed {
			Cow::Borrowed(blob)
		} else {
			Cow::Owned(decompress_ref(blob, self.compression)?)
		};

		let result = match self.format {
			TileFormat::PNG => parse_png_alpha(data.as_slice()),
			TileFormat::WEBP => parse_webp_alpha(data.as_slice()),
			TileFormat::JPG => AlphaInfo::NoAlpha,
			_ => AlphaInfo::Unknown,
		};

		Ok(result)
	}
}

/// Result of parsing image header for alpha channel information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AlphaInfo {
	/// Format has no alpha channel (JPEG, RGB PNG, etc.)
	NoAlpha,
	/// Format has alpha channel - need full decode to check values
	HasAlpha,
	/// Could not determine from header - need full decode
	Unknown,
}

/// Parse PNG header to determine alpha channel presence.
///
/// PNG IHDR chunk structure (RFC 2083):
/// - Bytes 0-7: PNG signature `\x89PNG\r\n\x1a\n`
/// - Bytes 12-15: Chunk type "IHDR"
/// - Byte 25: Color type (0=gray, 2=RGB, 3=indexed, 4=gray+alpha, 6=RGBA)
pub(super) fn parse_png_alpha(data: &[u8]) -> AlphaInfo {
	// Need at least 26 bytes for PNG signature + IHDR up to color type
	if data.len() < 26 {
		return AlphaInfo::Unknown;
	}

	// Verify PNG signature
	if &data[0..8] != b"\x89PNG\r\n\x1a\n" {
		return AlphaInfo::Unknown;
	}

	// Verify IHDR chunk type
	if &data[12..16] != b"IHDR" {
		return AlphaInfo::Unknown;
	}

	let color_type = data[25];
	match color_type {
		0 | 2 => AlphaInfo::NoAlpha,  // Grayscale or RGB
		4 | 6 => AlphaInfo::HasAlpha, // Grayscale+Alpha or RGBA
		// Color type 3 (indexed) would need tRNS chunk parsing
		_ => AlphaInfo::Unknown,
	}
}

/// Parse WebP header to determine alpha channel presence.
///
/// WebP format structure:
/// - Bytes 0-3: "RIFF"
/// - Bytes 8-11: "WEBP"
/// - Bytes 12-15: Chunk type (VP8, VP8L, or VP8X)
/// - For VP8X: Byte 20 bit 4 indicates alpha presence
pub(super) fn parse_webp_alpha(data: &[u8]) -> AlphaInfo {
	// Need at least 16 bytes for RIFF + WEBP + chunk header
	if data.len() < 16 {
		return AlphaInfo::Unknown;
	}

	// Verify RIFF/WEBP signature
	if &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
		return AlphaInfo::Unknown;
	}

	let chunk_type = &data[12..16];
	match chunk_type {
		b"VP8 " => AlphaInfo::NoAlpha,  // Lossy VP8 has no alpha
		b"VP8L" => AlphaInfo::HasAlpha, // Lossless can have alpha
		b"VP8X" => {
			// Extended format - check alpha flag at byte 20, bit 4
			if data.len() < 21 {
				return AlphaInfo::Unknown;
			}
			if data[20] & 0x10 != 0 {
				AlphaInfo::HasAlpha
			} else {
				AlphaInfo::NoAlpha
			}
		}
		_ => AlphaInfo::Unknown,
	}
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use crate::TileContent;

	use super::*;
	use rstest::rstest;
	use std::{collections::HashMap, sync::LazyLock};
	use versatiles_image::{DynamicImage, DynamicImageTraitConvert};

	static TEST_TILES: LazyLock<HashMap<String, Tile>> = LazyLock::new(|| {
		use versatiles_core::TileCompression::Uncompressed as U;
		use versatiles_core::TileFormat::{JPG, PNG, WEBP};

		fn create_image(colors: &str) -> DynamicImage {
			let mut data = Vec::new();
			let text = colors.replace(',', "");
			for color in text.chars() {
				let v = color.to_digit(16).unwrap() as u8 * 17;
				data.push(v);
			}
			DynamicImage::from_raw(2, 2, data).unwrap()
		}

		let mut m = HashMap::new();
		fn add(m: &mut HashMap<String, Tile>, name: &str, formats: &[TileFormat], colors: &str) {
			let image = create_image(colors);
			for &format in formats {
				let tile = Tile {
					blob: Some(image.to_blob(format, None, None).unwrap()),
					content: Some(TileContent::Raster(image.clone())),
					format,
					compression: U,
					format_quality: None,
					format_speed: None,
					transparency_cache: None,
				};
				m.insert(format!("{format}_{name}"), tile);
			}
		}

		add(&mut m, "gray", &[PNG, JPG], "0,4,8,F");
		add(&mut m, "gray_alpha_mixed", &[PNG], "0F,48,84,F0");
		add(&mut m, "gray_alpha_opaque", &[PNG], "0F,4F,8F,FF");
		add(&mut m, "gray_alpha_empty", &[PNG], "00,40,80,F0");
		add(&mut m, "rgb", &[PNG, JPG, WEBP], "F00,0F0,00F,888");
		add(&mut m, "rgba_mixed", &[PNG, WEBP], "F00F,0F08,00F4,8080");
		add(&mut m, "rgba_opaque", &[PNG, WEBP], "F00F,0F0F,00FF,888F");
		add(&mut m, "rgba_empty", &[PNG, WEBP], "F000,0F00,00F0,8880");

		let b = Blob::from(vec![0u8; 10]);
		m.insert("jpg_invalid".to_string(), Tile::from_blob(b.clone(), U, JPG));
		m.insert("png_invalid".to_string(), Tile::from_blob(b.clone(), U, PNG));
		m.insert("webp_invalid".to_string(), Tile::from_blob(b.clone(), U, WEBP));

		m
	});

	fn get_test_tile(name: &str) -> Tile {
		TEST_TILES
			.get(name)
			.unwrap_or_else(|| panic!("Test tile not found: {name}"))
			.clone()
	}

	/// Test header parsing on real encoded PNG and WebP blobs
	#[rstest]
	#[case("jpg_rgb", AlphaInfo::NoAlpha, false, true)]
	#[case("jpg_gray", AlphaInfo::NoAlpha, false, true)]
	#[case("jpg_invalid", AlphaInfo::Unknown, false, false)]
	#[case("png_rgb", AlphaInfo::NoAlpha, false, true)]
	#[case("png_rgba_opaque", AlphaInfo::NoAlpha, false, true)]
	#[case("png_rgba_empty", AlphaInfo::HasAlpha, true, false)]
	#[case("png_rgba_mixed", AlphaInfo::HasAlpha, false, false)]
	#[case("png_gray", AlphaInfo::NoAlpha, false, true)]
	#[case("png_gray_alpha_opaque", AlphaInfo::NoAlpha, false, true)]
	#[case("png_gray_alpha_mixed", AlphaInfo::HasAlpha, false, false)]
	#[case("png_gray_alpha_empty", AlphaInfo::HasAlpha, true, false)]
	#[case("png_invalid", AlphaInfo::Unknown, false, false)]
	#[case("webp_rgb", AlphaInfo::NoAlpha, false, true)]
	#[case("webp_rgba_opaque", AlphaInfo::NoAlpha, false, true)]
	#[case("webp_rgba_empty", AlphaInfo::HasAlpha, true, false)]
	#[case("webp_rgba_mixed", AlphaInfo::HasAlpha, false, false)]
	#[case("webp_invalid", AlphaInfo::Unknown, false, false)]
	fn header_parsing_on_real_blobs(
		#[case] name: &str,
		#[case] exp_info: AlphaInfo,
		#[case] exp_is_empty: bool,
		#[case] exp_is_opaque: bool,
	) -> Result<()> {
		use versatiles_core::TileFormat::{PNG, WEBP};

		let tile = get_test_tile(name);
		let slice = tile.blob.as_ref().unwrap().as_slice();
		let format = tile.format;

		match format {
			PNG => assert_eq!(parse_png_alpha(slice), exp_info, "{name}: PNG alpha info mismatch"),
			WEBP => assert_eq!(parse_webp_alpha(slice), exp_info, "{name}: WEBP alpha info mismatch"),
			_ => (),
		}

		if name.ends_with("_invalid") {
			return Ok(());
		}

		assert_eq!(
			tile.clone().check_alpha_from_header()?,
			exp_info,
			"{name}: header alpha info mismatch"
		);

		assert_eq!(
			tile.clone().compute_transparency()?,
			(exp_is_empty, exp_is_opaque),
			"{name}: transparency info mismatch"
		);

		assert_eq!(
			tile.clone().compute_transparency_uncached()?,
			(exp_is_empty, exp_is_opaque),
			"{name}: uncached transparency info mismatch"
		);

		assert_eq!(tile.clone().is_empty()?, exp_is_empty, "{name}: is_empty mismatch");

		assert_eq!(tile.clone().is_opaque()?, exp_is_opaque, "{name}: is_opaque mismatch");

		Ok(())
	}
}
