//! This module defines the `TileFormat` enum, representing various tile formats and their associated
//! extensions. It includes methods for converting between tile formats and file extensions, and
//! extracting the format from a filename.
//!
//! The `TileFormat` enum supports a variety of tile formats such as `AVIF`, `BIN`, `GEOJSON`, `JPG`,
//! `JSON`, `PBF`, `PNG`, `SVG`, `TOPOJSON`, and `WEBP`. Each variant has a method to get its corresponding
//! file extension and to extract the format from a filename.
//!
//! # Examples
//!
//! ```
//! use versatiles_core::types::TileFormat;
//!
//! // Getting the file extension for a tile format
//! let format = TileFormat::PNG;
//! assert_eq!(format.extension(), ".png");
//!
//! // Extracting the tile format from a filename
//! let mut filename = String::from("map.pbf");
//! let format = TileFormat::from_filename(&mut filename).unwrap();
//! assert_eq!(format, TileFormat::PBF);
//! assert_eq!(filename, "map");
//! ```

use anyhow::{bail, Result};
#[cfg(feature = "cli")]
use clap::ValueEnum;
use std::fmt::Display;

// Enum representing supported tile formats
#[allow(clippy::upper_case_acronyms)]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TileFormat {
	AVIF,
	BIN,
	GEOJSON,
	JPG,
	JSON,
	PBF,
	PNG,
	SVG,
	TOPOJSON,
	WEBP,
}

impl Display for TileFormat {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(match self {
			TileFormat::AVIF => "avif",
			TileFormat::BIN => "bin",
			TileFormat::GEOJSON => "geojson",
			TileFormat::JPG => "jpg",
			TileFormat::JSON => "json",
			TileFormat::PBF => "pbf",
			TileFormat::PNG => "png",
			TileFormat::SVG => "svg",
			TileFormat::TOPOJSON => "topojson",
			TileFormat::WEBP => "webp",
		})
	}
}

impl TileFormat {
	pub fn extension(&self) -> &str {
		match self {
			TileFormat::AVIF => ".avif",
			TileFormat::BIN => ".bin",
			TileFormat::GEOJSON => ".geojson",
			TileFormat::JPG => ".jpg",
			TileFormat::JSON => ".json",
			TileFormat::PBF => ".pbf",
			TileFormat::PNG => ".png",
			TileFormat::SVG => ".svg",
			TileFormat::TOPOJSON => ".topojson",
			TileFormat::WEBP => ".webp",
		}
	}

	pub fn from_filename(filename: &mut String) -> Option<TileFormat> {
		if let Some(index) = filename.rfind('.') {
			let format = match filename.get(index..).unwrap() {
				".avif" => TileFormat::AVIF,
				".bin" => TileFormat::BIN,
				".geojson" => TileFormat::GEOJSON,
				".jpg" => TileFormat::JPG,
				".jpeg" => TileFormat::JPG,
				".json" => TileFormat::JSON,
				".pbf" => TileFormat::PBF,
				".png" => TileFormat::PNG,
				".svg" => TileFormat::SVG,
				".topojson" => TileFormat::TOPOJSON,
				".webp" => TileFormat::WEBP,
				_ => return None,
			};
			filename.truncate(index);
			return Some(format);
		}
		None
	}

	pub fn parse_str(value: &str) -> Result<Self> {
		Ok(match value.to_lowercase().trim_matches([' ', '.']) {
			"avif" => TileFormat::AVIF,
			"bin" => TileFormat::BIN,
			"geojson" => TileFormat::GEOJSON,
			"jpeg" => TileFormat::JPG,
			"jpg" => TileFormat::JPG,
			"json" => TileFormat::JSON,
			"pbf" => TileFormat::PBF,
			"png" => TileFormat::PNG,
			"svg" => TileFormat::SVG,
			"topojson" => TileFormat::TOPOJSON,
			"webp" => TileFormat::WEBP,
			_ => bail!("Unknown tile format. Expected: PBF"),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_format_to_extension() {
		fn test(format: TileFormat, expected_extension: &str) {
			assert_eq!(
				format.extension(),
				expected_extension,
				"Extension does not match {expected_extension}"
			);
		}

		test(TileFormat::AVIF, ".avif");
		test(TileFormat::BIN, ".bin");
		test(TileFormat::GEOJSON, ".geojson");
		test(TileFormat::JPG, ".jpg");
		test(TileFormat::JSON, ".json");
		test(TileFormat::PBF, ".pbf");
		test(TileFormat::PNG, ".png");
		test(TileFormat::SVG, ".svg");
		test(TileFormat::TOPOJSON, ".topojson");
		test(TileFormat::WEBP, ".webp");
	}

	#[test]
	fn test_extract_format() {
		fn test(expected_format: Option<TileFormat>, filename: &str, rest: &str) {
			let mut filename_string = String::from(filename);
			assert_eq!(
				TileFormat::from_filename(&mut filename_string),
				expected_format,
				"Extracted format does not match expected for filename: {filename}"
			);
			assert_eq!(
				filename_string, rest,
				"Filename remainder does not match expected for filename: {filename}"
			);
		}

		test(Some(TileFormat::AVIF), "image.avif", "image");
		test(None, "archive.zip", "archive.zip");
		test(Some(TileFormat::BIN), "binary.bin", "binary");
		test(None, "noextensionfile", "noextensionfile");
		test(None, "unknown.ext", "unknown.ext");
		test(Some(TileFormat::GEOJSON), "data.geojson", "data");
		test(Some(TileFormat::JPG), "image.jpeg", "image");
		test(Some(TileFormat::JPG), "image.jpg", "image");
		test(Some(TileFormat::JSON), "document.json", "document");
		test(Some(TileFormat::PBF), "map.pbf", "map");
		test(Some(TileFormat::PNG), "picture.png", "picture");
		test(Some(TileFormat::SVG), "diagram.svg", "diagram");
		test(Some(TileFormat::SVG), "vector.svg", "vector");
		test(
			Some(TileFormat::TOPOJSON),
			"topography.topojson",
			"topography",
		);
		test(Some(TileFormat::WEBP), "photo.webp", "photo");
	}
}
