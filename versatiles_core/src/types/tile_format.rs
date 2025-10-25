//! This module defines the `TileFormat` enum, representing various tile formats and their associated
//! extensions. It includes methods for converting between tile formats and file extensions, and
//! extracting the format from a filename.
//!
//! The `TileFormat` enum supports a variety of tile formats such as `AVIF`, `BIN`, `GEOJSON`, `JPG`,
//! `JSON`, `PBF`, `PNG`, `SVG`, `TOPOJSON`, and `WEBP`. Each variant provides its canonical file extension
//! and can be derived from a filename or string representation.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::TileFormat;
//!
//! // Getting the file extension for a tile format
//! let format = TileFormat::PNG;
//! assert_eq!(format.as_extension(), ".png");
//!
//! // Extracting the tile format from a filename
//! let mut filename = String::from("map.pbf");
//! let format = TileFormat::from_filename(&mut filename).unwrap();
//! assert_eq!(format, TileFormat::MVT);
//! assert_eq!(filename, "map");
//!
//! // Parsing a tile format from a string (case-insensitive)
//! let format = TileFormat::parse_str("JPEG").unwrap();
//! assert_eq!(format, TileFormat::JPG);
//! ```

use super::TileType;
use anyhow::{Result, bail};
#[cfg(feature = "cli")]
use clap::ValueEnum;
use std::{
	fmt::{Display, Formatter},
	path::Path,
};

/// Enum representing supported tile formats.
///
/// Each variant corresponds to a common file extension used for map tiles,
/// images, or related data formats. Variants like `JPG` also map from
/// alternative extensions (e.g., `.jpeg`).
///
/// # Variants
/// - `AVIF` - AVIF image format
/// - `BIN` - Raw binary data
/// - `GEOJSON` - `GeoJSON` vector data
/// - `JPG` - JPEG image format (including `.jpeg`)
/// - `JSON` - Generic JSON data
/// - `PBF` - Mapbox Vector Tile in Protocol Buffer format
/// - `PNG` - PNG image format
/// - `SVG` - SVG image format
/// - `TOPOJSON` - `TopoJSON` vector data
/// - `WEBP` - WEBP image format
#[allow(clippy::upper_case_acronyms)]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TileFormat {
	AVIF,
	#[default]
	BIN,
	GEOJSON,
	JPG,
	JSON,
	MVT,
	PNG,
	SVG,
	TOPOJSON,
	WEBP,
}

impl TileFormat {
	/// Returns a lowercase string identifier for this tile format.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileFormat;
	/// let format = TileFormat::PNG;
	/// assert_eq!(format.as_str(), "png");
	/// ```
	#[must_use]
	pub fn as_str(&self) -> &str {
		match self {
			TileFormat::AVIF => "avif",
			TileFormat::BIN => "bin",
			TileFormat::GEOJSON => "geojson",
			TileFormat::JPG => "jpg",
			TileFormat::JSON => "json",
			TileFormat::MVT => "mvt",
			TileFormat::PNG => "png",
			TileFormat::SVG => "svg",
			TileFormat::TOPOJSON => "topojson",
			TileFormat::WEBP => "webp",
		}
	}

	pub fn try_from_str(value: &str) -> Result<Self> {
		Ok(match value.to_lowercase().trim() {
			"avif" => TileFormat::AVIF,
			"bin" => TileFormat::BIN,
			"geojson" => TileFormat::GEOJSON,
			"jpeg" | "jpg" => TileFormat::JPG,
			"json" => TileFormat::JSON,
			"pbf" | "mvt" => TileFormat::MVT,
			"png" => TileFormat::PNG,
			"svg" => TileFormat::SVG,
			"topojson" => TileFormat::TOPOJSON,
			"webp" => TileFormat::WEBP,
			_ => bail!("Unknown tile format: '{value}'"),
		})
	}

	pub fn try_from_path(path: &Path) -> Result<Self> {
		Self::try_from_str(path.extension().and_then(|s| s.to_str()).unwrap_or_default())
	}

	/// Returns a string describing the broad data type of this tile format.
	///
	/// Possible values are `"image"`, `"vector"`, or `"unknown"`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileFormat;
	/// let format = TileFormat::GEOJSON;
	/// assert_eq!(format.as_type_str(), "vector");
	/// ```
	#[must_use]
	pub fn as_type_str(&self) -> &str {
		match self {
			TileFormat::AVIF | TileFormat::JPG | TileFormat::PNG | TileFormat::SVG | TileFormat::WEBP => "image",
			TileFormat::BIN | TileFormat::JSON => "unknown",
			TileFormat::GEOJSON | TileFormat::MVT | TileFormat::TOPOJSON => "vector",
		}
	}

	/// Returns a MIME type string typically associated with this tile format.
	///
	/// These MIME types are approximate and may vary based on context.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileFormat;
	/// let format = TileFormat::PNG;
	/// assert_eq!(format.as_mime_str(), "image/png");
	/// ```
	#[must_use]
	pub fn as_mime_str(&self) -> &str {
		match self {
			TileFormat::BIN => "application/octet-stream",
			TileFormat::PNG => "image/png",
			TileFormat::JPG => "image/jpeg",
			TileFormat::WEBP => "image/webp",
			TileFormat::AVIF => "image/avif",
			TileFormat::SVG => "image/svg+xml",
			TileFormat::MVT => "vnd.mapbox-vector-tile",
			TileFormat::GEOJSON => "application/geo+json",
			TileFormat::TOPOJSON => "application/topo+json",
			TileFormat::JSON => "application/json",
		}
	}

	pub fn try_from_mime(mime: &str) -> Result<Self> {
		Ok(match mime.to_lowercase().as_str() {
			"application/octet-stream" => TileFormat::BIN,
			"image/png" => TileFormat::PNG,
			"image/jpeg" => TileFormat::JPG,
			"image/webp" => TileFormat::WEBP,
			"image/avif" => TileFormat::AVIF,
			"image/svg+xml" => TileFormat::SVG,
			"vnd.mapbox-vector-tile" => TileFormat::MVT,
			"application/geo+json" => TileFormat::GEOJSON,
			"application/topo+json" => TileFormat::TOPOJSON,
			"application/json" => TileFormat::JSON,
			_ => bail!("Unknown MIME type: '{mime}'"),
		})
	}

	/// Returns the canonical file extension for this tile format (with a leading dot).
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileFormat;
	/// let format = TileFormat::SVG;
	/// assert_eq!(format.as_extension(), ".svg");
	/// ```
	#[must_use]
	pub fn as_extension(&self) -> &str {
		match self {
			TileFormat::AVIF => ".avif",
			TileFormat::BIN => ".bin",
			TileFormat::GEOJSON => ".geojson",
			TileFormat::JPG => ".jpg",
			TileFormat::JSON => ".json",
			TileFormat::MVT => ".pbf",
			TileFormat::PNG => ".png",
			TileFormat::SVG => ".svg",
			TileFormat::TOPOJSON => ".topojson",
			TileFormat::WEBP => ".webp",
		}
	}

	/// Attempts to extract a `TileFormat` from the file extension in `filename`.
	///
	/// If a matching extension (e.g. `.pbf` or `.jpeg`) is found, the `TileFormat`
	/// is returned and the filename is truncated to remove the extension.
	/// If no known extension is found, returns `None`.
	///
	/// # Arguments
	///
	/// * `filename` - A mutable `String` representing a filename.\
	///   If an extension is matched, the filename is truncated (the extension removed).
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileFormat;
	///
	/// let mut filename = String::from("picture.jpeg");
	/// let format = TileFormat::from_filename(&mut filename);
	/// assert_eq!(Some(TileFormat::JPG), format);
	/// assert_eq!("picture", filename);
	///
	/// let mut unknown = String::from("file.abc");
	/// let format_none = TileFormat::from_filename(&mut unknown);
	/// assert_eq!(None, format_none);
	/// assert_eq!("file.abc", unknown);
	/// ```
	pub fn from_filename(filename: &mut String) -> Option<Self> {
		if let Some(index) = filename.rfind('.') {
			let extension = filename[index..].to_lowercase();
			let format = match extension.as_str() {
				".avif" => TileFormat::AVIF,
				".bin" => TileFormat::BIN,
				".geojson" => TileFormat::GEOJSON,
				".jpg" | ".jpeg" => TileFormat::JPG,
				".json" => TileFormat::JSON,
				".pbf" => TileFormat::MVT,
				".png" => TileFormat::PNG,
				".svg" => TileFormat::SVG,
				".topojson" => TileFormat::TOPOJSON,
				".webp" => TileFormat::WEBP,
				_ => return None,
			};
			filename.truncate(index);
			Some(format)
		} else {
			None
		}
	}

	/// Attempts to parse a `TileFormat` from a string, ignoring leading dots and whitespace.
	///
	/// For instance, `".jpeg"`, `" JPeG "`, or `"svg"` all resolve to recognized tile formats.
	///
	/// # Arguments
	///
	/// * `value` - The string to parse.
	///
	/// # Errors
	///
	/// Returns an error if the format is not recognized.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileFormat;
	///
	/// // Recognizes .jpeg as JPG.
	/// let format = TileFormat::parse_str(".jpeg").unwrap();
	/// assert_eq!(format, TileFormat::JPG);
	///
	/// // Returns an error if unknown.
	/// assert!(TileFormat::parse_str(".abc").is_err());
	/// ```
	pub fn parse_str(value: &str) -> Result<Self> {
		Ok(match value.to_lowercase().trim_matches([' ', '.']) {
			"avif" => TileFormat::AVIF,
			"bin" => TileFormat::BIN,
			"geojson" => TileFormat::GEOJSON,
			"jpeg" | "jpg" => TileFormat::JPG,
			"json" => TileFormat::JSON,
			"mvt" => TileFormat::MVT,
			"png" => TileFormat::PNG,
			"svg" => TileFormat::SVG,
			"topojson" => TileFormat::TOPOJSON,
			"webp" => TileFormat::WEBP,
			_ => bail!("Unknown tile format: '{}'", value.trim()),
		})
	}

	#[must_use]
	pub fn to_type(&self) -> TileType {
		use TileFormat::*;
		use TileType::*;
		match self {
			AVIF | PNG | JPG | WEBP => Raster,
			MVT => Vector,
			BIN | GEOJSON | JSON | SVG | TOPOJSON => Unknown,
		}
	}
}

impl TryFrom<&str> for TileFormat {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self> {
		Self::try_from_str(value)
	}
}

impl Display for TileFormat {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(self.as_str())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn should_return_correct_extension_for_format() {
		#[rustfmt::skip]
        let cases = vec![
            (TileFormat::AVIF, ".avif"),
            (TileFormat::BIN, ".bin"),
            (TileFormat::GEOJSON, ".geojson"),
            (TileFormat::JPG, ".jpg"),
            (TileFormat::JSON, ".json"),
            (TileFormat::MVT, ".pbf"),
            (TileFormat::PNG, ".png"),
            (TileFormat::SVG, ".svg"),
            (TileFormat::TOPOJSON, ".topojson"),
            (TileFormat::WEBP, ".webp"),
        ];

		for (format, expected) in cases {
			assert_eq!(format.as_extension(), expected);
		}
	}

	#[test]
	fn should_extract_correct_format_and_truncate_filename_when_extension_found() {
		struct Case(&'static str, Option<TileFormat>, &'static str);

		let cases = vec![
			Case("image.avif", Some(TileFormat::AVIF), "image"),
			Case("archive.zip", None, "archive.zip"),
			Case("binary.bin", Some(TileFormat::BIN), "binary"),
			Case("noextensionfile", None, "noextensionfile"),
			Case("unknown.ext", None, "unknown.ext"),
			Case("data.geojson", Some(TileFormat::GEOJSON), "data"),
			Case("image.jpeg", Some(TileFormat::JPG), "image"),
			Case("image.jpg", Some(TileFormat::JPG), "image"),
			Case("document.json", Some(TileFormat::JSON), "document"),
			Case("map.pbf", Some(TileFormat::MVT), "map"),
			Case("picture.png", Some(TileFormat::PNG), "picture"),
			Case("diagram.svg", Some(TileFormat::SVG), "diagram"),
			Case("vector.SVG", Some(TileFormat::SVG), "vector"),
			Case("topography.topojson", Some(TileFormat::TOPOJSON), "topography"),
			Case("photo.webp", Some(TileFormat::WEBP), "photo"),
		];

		for case in cases {
			let mut filename = String::from(case.0);
			let format = TileFormat::from_filename(&mut filename);
			assert_eq!(format, case.1);
			assert_eq!(filename, case.2);
		}
	}

	#[test]
	fn should_parse_str_into_tileformat() {
		struct Case(&'static str, Option<TileFormat>);

		let cases = vec![
			Case("avif", Some(TileFormat::AVIF)),
			Case(".bin", Some(TileFormat::BIN)),
			Case("GEOJSON", Some(TileFormat::GEOJSON)),
			Case("jpeg", Some(TileFormat::JPG)),
			Case("jpg", Some(TileFormat::JPG)),
			Case(".json", Some(TileFormat::JSON)),
			Case(" mvt ", Some(TileFormat::MVT)),
			Case("png", Some(TileFormat::PNG)),
			Case(".topojson", Some(TileFormat::TOPOJSON)),
			Case(".webp", Some(TileFormat::WEBP)),
			Case("unknown", None),
		];

		for case in cases {
			let result = TileFormat::parse_str(case.0);
			match case.1 {
				Some(expected_format) => {
					assert_eq!(result.unwrap(), expected_format);
				}
				None => {
					assert!(result.is_err());
				}
			}
		}
	}

	#[test]
	fn should_provide_meaningful_strings_for_debug_and_display() {
		let format = TileFormat::PNG;
		assert!(format!("{format:?}").contains("PNG"));
		assert_eq!(format!("{format}"), "png");
	}

	#[test]
	fn should_return_lowercase_string_for_as_str() {
		#![allow(clippy::enum_variant_names)]
		#[rustfmt::skip]
        let cases = vec![
            (TileFormat::AVIF, "avif"),
            (TileFormat::BIN, "bin"),
            (TileFormat::GEOJSON, "geojson"),
            (TileFormat::JPG, "jpg"),
            (TileFormat::JSON, "json"),
            (TileFormat::MVT, "mvt"),
            (TileFormat::PNG, "png"),
            (TileFormat::SVG, "svg"),
            (TileFormat::TOPOJSON, "topojson"),
            (TileFormat::WEBP, "webp"),
        ];
		for (format, expected) in cases {
			assert_eq!(format.as_str(), expected);
		}
	}

	#[test]
	fn should_return_correct_type_str() {
		assert_eq!(TileFormat::PNG.as_type_str(), "image");
		assert_eq!(TileFormat::MVT.as_type_str(), "vector");
		assert_eq!(TileFormat::BIN.as_type_str(), "unknown");
	}

	#[test]
	fn should_return_correct_mime_str() {
		assert_eq!(TileFormat::PNG.as_mime_str(), "image/png");
		assert_eq!(TileFormat::JPG.as_mime_str(), "image/jpeg");
		assert_eq!(TileFormat::GEOJSON.as_mime_str(), "application/geo+json");
	}

	#[test]
	fn should_try_from_str_parse_valid_and_error_invalid() {
		assert_eq!(TileFormat::try_from_str("png").unwrap(), TileFormat::PNG);
		assert!(TileFormat::try_from_str("invalid").is_err());
	}

	#[test]
	fn should_try_from_mime_parse_valid_and_error_invalid() {
		assert_eq!(TileFormat::try_from_mime("image/webp").unwrap(), TileFormat::WEBP);
		assert!(TileFormat::try_from_mime("application/x-unknown").is_err());
	}

	#[test]
	fn should_get_type_return_expected() {
		use super::TileType::*;
		assert_eq!(TileFormat::PNG.to_type(), Raster);
		assert_eq!(TileFormat::MVT.to_type(), Vector);
		assert_eq!(TileFormat::BIN.to_type(), Unknown);
	}
}
