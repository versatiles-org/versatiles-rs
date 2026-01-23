//! Color parsing utilities.
//!
//! This module provides functions for parsing color values from various string formats.

use anyhow::{Result, bail};

/// Parses a hex color string into RGB or RGBA bytes.
///
/// Supports formats:
/// - "RGB" (3 chars) -> expands to RRGGBB
/// - "RGBA" (4 chars) -> expands to RRGGBBAA
/// - "RRGGBB" (6 chars)
/// - "RRGGBBAA" (8 chars)
///
/// An optional leading `#` is stripped.
///
/// # Examples
///
/// ```
/// use versatiles_image::color::parse_hex_color;
///
/// assert_eq!(parse_hex_color("FF5733").unwrap(), vec![255, 87, 51]);
/// assert_eq!(parse_hex_color("#F00").unwrap(), vec![255, 0, 0]);
/// assert_eq!(parse_hex_color("FF573380").unwrap(), vec![255, 87, 51, 128]);
/// ```
pub fn parse_hex_color(hex: &str) -> Result<Vec<u8>> {
	let hex = hex.trim_start_matches('#');

	let expanded = match hex.len() {
		3 => {
			// RGB -> RRGGBB
			let chars: Vec<char> = hex.chars().collect();
			format!(
				"{}{}{}{}{}{}",
				chars[0], chars[0], chars[1], chars[1], chars[2], chars[2]
			)
		}
		4 => {
			// RGBA -> RRGGBBAA
			let chars: Vec<char> = hex.chars().collect();
			format!(
				"{}{}{}{}{}{}{}{}",
				chars[0], chars[0], chars[1], chars[1], chars[2], chars[2], chars[3], chars[3]
			)
		}
		6 | 8 => hex.to_string(),
		_ => bail!("Invalid hex color '{hex}': expected 3, 4, 6, or 8 hex characters"),
	};

	let bytes: Result<Vec<u8>, _> = (0..expanded.len())
		.step_by(2)
		.map(|i| u8::from_str_radix(&expanded[i..i + 2], 16))
		.collect();

	bytes.map_err(|e| anyhow::anyhow!("Invalid hex color '{hex}': {e}"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_hex_color_rgb_short() {
		assert_eq!(parse_hex_color("F00").unwrap(), vec![255, 0, 0]);
		assert_eq!(parse_hex_color("0F0").unwrap(), vec![0, 255, 0]);
		assert_eq!(parse_hex_color("00F").unwrap(), vec![0, 0, 255]);
	}

	#[test]
	fn test_parse_hex_color_rgba_short() {
		assert_eq!(parse_hex_color("F00F").unwrap(), vec![255, 0, 0, 255]);
		assert_eq!(parse_hex_color("0F08").unwrap(), vec![0, 255, 0, 136]);
	}

	#[test]
	fn test_parse_hex_color_rgb() {
		assert_eq!(parse_hex_color("FF5733").unwrap(), vec![255, 87, 51]);
		assert_eq!(parse_hex_color("000000").unwrap(), vec![0, 0, 0]);
		assert_eq!(parse_hex_color("FFFFFF").unwrap(), vec![255, 255, 255]);
	}

	#[test]
	fn test_parse_hex_color_rgba() {
		assert_eq!(parse_hex_color("FF573380").unwrap(), vec![255, 87, 51, 128]);
		assert_eq!(parse_hex_color("000000FF").unwrap(), vec![0, 0, 0, 255]);
	}

	#[test]
	fn test_parse_hex_color_with_hash() {
		assert_eq!(parse_hex_color("#FF5733").unwrap(), vec![255, 87, 51]);
		assert_eq!(parse_hex_color("#F00").unwrap(), vec![255, 0, 0]);
	}

	#[test]
	fn test_parse_hex_color_invalid() {
		assert!(parse_hex_color("GG0000").is_err());
		assert!(parse_hex_color("FF").is_err());
		assert!(parse_hex_color("FF5733FF0").is_err());
		assert!(parse_hex_color("FFFFF").is_err());
	}
}
