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
//! let format = TileFormat::try_from_str("JPEG").unwrap();
//! assert_eq!(format, TileFormat::JPG);
//! ```

use super::TileType;
use TileFormat::*;
use anyhow::{Result, bail};
#[cfg(feature = "cli")]
use clap::ValueEnum;
use enumset::EnumSetType;
use std::{
	fmt::{Display, Formatter},
	path::Path,
};
use versatiles_derive::context;

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
#[derive(Debug, Default, PartialOrd, Ord, EnumSetType)]
pub enum TileFormat {
	/// AVIF image format.
	AVIF,
	#[default]
	/// Raw binary data.
	BIN,
	/// GeoJSON vector data.
	GEOJSON,
	/// JPEG image format (including `.jpeg`).
	JPG,
	/// Generic JSON data.
	JSON,
	/// Mapbox Vector Tile in Protocol Buffer format.
	MVT,
	/// PNG image format.
	PNG,
	/// SVG image format.
	SVG,
	/// TopoJSON vector data.
	TOPOJSON,
	/// WEBP image format.
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
			AVIF => "avif",
			BIN => "bin",
			GEOJSON => "geojson",
			JPG => "jpg",
			JSON => "json",
			MVT => "mvt",
			PNG => "png",
			SVG => "svg",
			TOPOJSON => "topojson",
			WEBP => "webp",
		}
	}

	#[context("Could not convert string '{value}' to TileFormat")]
	pub fn try_from_str(value: &str) -> Result<Self> {
		Ok(match value.to_lowercase().trim_matches([' ', '.']) {
			"avif" => AVIF,
			"bin" => BIN,
			"geojson" => GEOJSON,
			"jpeg" | "jpg" => JPG,
			"json" => JSON,
			"pbf" | "mvt" => MVT,
			"png" => PNG,
			"svg" => SVG,
			"topojson" => TOPOJSON,
			"webp" => WEBP,
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
			BIN => "application/octet-stream",
			PNG => "image/png",
			JPG => "image/jpeg",
			WEBP => "image/webp",
			AVIF => "image/avif",
			SVG => "image/svg+xml",
			MVT => "vnd.mapbox-vector-tile",
			GEOJSON => "application/geo+json",
			TOPOJSON => "application/topo+json",
			JSON => "application/json",
		}
	}

	pub fn try_from_mime(mime: &str) -> Result<Self> {
		Ok(match mime.to_lowercase().as_str() {
			"application/octet-stream" => BIN,
			"image/png" => PNG,
			"image/jpeg" => JPG,
			"image/webp" => WEBP,
			"image/avif" => AVIF,
			"image/svg+xml" => SVG,
			"vnd.mapbox-vector-tile" => MVT,
			"application/geo+json" => GEOJSON,
			"application/topo+json" => TOPOJSON,
			"application/json" => JSON,
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
			AVIF => ".avif",
			BIN => ".bin",
			GEOJSON => ".geojson",
			JPG => ".jpg",
			JSON => ".json",
			MVT => ".pbf",
			PNG => ".png",
			SVG => ".svg",
			TOPOJSON => ".topojson",
			WEBP => ".webp",
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
				".avif" => AVIF,
				".bin" => BIN,
				".geojson" => GEOJSON,
				".jpg" | ".jpeg" => JPG,
				".json" => JSON,
				".pbf" => MVT,
				".png" => PNG,
				".svg" => SVG,
				".topojson" => TOPOJSON,
				".webp" => WEBP,
				_ => return None,
			};
			filename.truncate(index);
			Some(format)
		} else {
			None
		}
	}

	pub fn to_type(&self) -> TileType {
		use TileType::*;
		match self {
			AVIF | PNG | JPG | WEBP => Raster,
			MVT => Vector,
			BIN | GEOJSON | JSON | SVG | TOPOJSON => Unknown,
		}
	}

	pub fn is_raster(&self) -> bool {
		self.to_type() == TileType::Raster
	}

	pub fn is_vector(&self) -> bool {
		self.to_type() == TileType::Vector
	}
}

impl TryFrom<u8> for TileFormat {
	type Error = anyhow::Error;
	fn try_from(value: u8) -> Result<Self> {
		Ok(match value {
			0 => AVIF,
			1 => BIN,
			2 => GEOJSON,
			3 => JPG,
			4 => JSON,
			5 => MVT,
			6 => PNG,
			7 => SVG,
			8 => TOPOJSON,
			9 => WEBP,
			_ => bail!("Unknown tile format value: {value}"),
		})
	}
}

impl From<TileFormat> for u8 {
	fn from(format: TileFormat) -> u8 {
		match format {
			AVIF => 0,
			BIN => 1,
			GEOJSON => 2,
			JPG => 3,
			JSON => 4,
			MVT => 5,
			PNG => 6,
			SVG => 7,
			TOPOJSON => 8,
			WEBP => 9,
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
	use enumset::EnumSet;
	use rstest::rstest;
	use std::collections::HashSet;

	#[test]
	fn test_format_conversion() {
		let mut all_bytes = (0..255).collect::<HashSet<u8>>();
		for format in EnumSet::<TileFormat>::all() {
			let byte: u8 = format.into();
			let parsed = TileFormat::try_from(byte).unwrap();
			assert_eq!(format, parsed);
			all_bytes.remove(&byte);
		}

		for byte in all_bytes {
			assert!(TileFormat::try_from(byte).is_err());
		}
	}

	#[test]
	fn should_return_correct_extension_for_format() {
		let cases = vec![
			(AVIF, ".avif"),
			(BIN, ".bin"),
			(GEOJSON, ".geojson"),
			(JPG, ".jpg"),
			(JSON, ".json"),
			(MVT, ".pbf"),
			(PNG, ".png"),
			(SVG, ".svg"),
			(TOPOJSON, ".topojson"),
			(WEBP, ".webp"),
		];

		for (format, expected) in cases {
			assert_eq!(format.as_extension(), expected);
		}
	}

	#[rstest]
	#[case("image.avif", Some(AVIF), "image")]
	#[case("archive.zip", None, "archive.zip")]
	#[case("binary.bin", Some(BIN), "binary")]
	#[case("noextensionfile", None, "noextensionfile")]
	#[case("unknown.ext", None, "unknown.ext")]
	#[case("data.geojson", Some(GEOJSON), "data")]
	#[case("image.jpeg", Some(JPG), "image")]
	#[case("image.jpg", Some(JPG), "image")]
	#[case("document.json", Some(JSON), "document")]
	#[case("map.pbf", Some(MVT), "map")]
	#[case("picture.png", Some(PNG), "picture")]
	#[case("diagram.svg", Some(SVG), "diagram")]
	#[case("vector.SVG", Some(SVG), "vector")]
	#[case("topography.topojson", Some(TOPOJSON), "topography")]
	#[case("photo.webp", Some(WEBP), "photo")]
	fn should_extract_correct_format_and_truncate_filename_when_extension_found(
		#[case] filename: &str,
		#[case] expected_format: Option<TileFormat>,
		#[case] expected_remainder: &str,
	) {
		let mut filename = String::from(filename);
		let format = TileFormat::from_filename(&mut filename);
		assert_eq!(format, expected_format);
		assert_eq!(filename, expected_remainder);
	}

	#[rstest]
	#[case("avif", Some(AVIF))]
	#[case(".bin", Some(BIN))]
	#[case("GEOJSON", Some(GEOJSON))]
	#[case("jpeg", Some(JPG))]
	#[case("jpg", Some(JPG))]
	#[case(".json", Some(JSON))]
	#[case(" mvt ", Some(MVT))]
	#[case("png", Some(PNG))]
	#[case(".topojson", Some(TOPOJSON))]
	#[case(".webp", Some(WEBP))]
	#[case("unknown", None)]
	fn should_parse_str_into_tileformat(#[case] input: &str, #[case] expected: Option<TileFormat>) {
		let result = TileFormat::try_from_str(input);
		match expected {
			Some(expected_format) => {
				assert_eq!(result.unwrap(), expected_format);
			}
			None => {
				assert!(result.is_err());
			}
		}
	}

	#[test]
	fn should_provide_meaningful_strings_for_debug_and_display() {
		let format = TileFormat::PNG;
		assert!(format!("{format:?}").contains("PNG"));
		assert_eq!(format!("{format}"), "png");
	}

	#[rstest]
	#[case(AVIF, "avif")]
	#[case(BIN, "bin")]
	#[case(GEOJSON, "geojson")]
	#[case(JPG, "jpg")]
	#[case(JSON, "json")]
	#[case(MVT, "mvt")]
	#[case(PNG, "png")]
	#[case(SVG, "svg")]
	#[case(TOPOJSON, "topojson")]
	#[case(WEBP, "webp")]
	fn should_return_lowercase_string_for_as_str(#[case] format: TileFormat, #[case] expected: &str) {
		assert_eq!(format.as_str(), expected);
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

		assert_eq!(PNG.to_type(), Raster);
		assert_eq!(MVT.to_type(), Vector);
		assert_eq!(BIN.to_type(), Unknown);
	}
}
