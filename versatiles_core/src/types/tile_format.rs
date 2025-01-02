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
//! use versatiles::types::TileFormat;
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
//!
//! // Parsing a tile format from a string (case-insensitive)
//! let format = TileFormat::parse_str("JPEG").unwrap();
//! assert_eq!(format, TileFormat::JPG);
//! ```

use anyhow::{bail, Result};
#[cfg(feature = "cli")]
use clap::ValueEnum;
use std::fmt::{Display, Formatter};

/// Enum representing supported tile formats.
///
/// Each variant corresponds to a common file extension used for map tiles,
/// images, or related data formats. Variants like `JPG` also map from
/// alternative extensions (e.g., `.jpeg`).
///
/// # Variants
/// - `AVIF` - AVIF image format
/// - `BIN` - Raw binary data
/// - `GEOJSON` - GeoJSON vector data
/// - `JPG` - JPEG image format (including `.jpeg`)
/// - `JSON` - Generic JSON data
/// - `PBF` - Mapbox Vector Tile in Protocol Buffer format
/// - `PNG` - PNG image format
/// - `SVG` - SVG image format
/// - `TOPOJSON` - TopoJSON vector data
/// - `WEBP` - WEBP image format
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

impl TileFormat {
	/// Returns a lowercase string identifier for this tile format.
	///
	/// # Examples
	/// ```
	/// use versatiles::types::TileFormat;
	/// let format = TileFormat::PNG;
	/// assert_eq!(format.as_str(), "png");
	/// ```
	pub fn as_str(&self) -> &str {
		match self {
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
		}
	}

	/// Returns a string describing the broad data type of this tile format.
	///
	/// Possible values are `"image"`, `"vector"`, or `"unknown"`.
	///
	/// # Examples
	/// ```
	/// use versatiles::types::TileFormat;
	/// let format = TileFormat::GEOJSON;
	/// assert_eq!(format.as_type_str(), "vector");
	/// ```
	pub fn as_type_str(&self) -> &str {
		match self {
			TileFormat::AVIF | TileFormat::JPG | TileFormat::PNG | TileFormat::SVG | TileFormat::WEBP => "image",
			TileFormat::BIN | TileFormat::JSON => "unknown",
			TileFormat::GEOJSON | TileFormat::PBF | TileFormat::TOPOJSON => "vector",
		}
	}

	/// Returns a MIME type string typically associated with this tile format.
	///
	/// These MIME types are approximate and may vary based on context.
	///
	/// # Examples
	/// ```
	/// use versatiles::types::TileFormat;
	/// let format = TileFormat::PNG;
	/// assert_eq!(format.as_mime_str(), "image/png");
	/// ```
	pub fn as_mime_str(&self) -> &str {
		match self {
			TileFormat::BIN => "application/octet-stream",
			TileFormat::PNG => "image/png",
			TileFormat::JPG => "image/jpeg",
			TileFormat::WEBP => "image/webp",
			TileFormat::AVIF => "image/avif",
			TileFormat::SVG => "image/svg+xml",
			TileFormat::PBF => "application/x-protobuf",
			TileFormat::GEOJSON => "application/geo+json",
			TileFormat::TOPOJSON => "application/topo+json",
			TileFormat::JSON => "application/json",
		}
	}

	/// Returns the canonical file extension for this tile format (with a leading dot).
	///
	/// # Examples
	/// ```
	/// use versatiles::types::TileFormat;
	/// let format = TileFormat::SVG;
	/// assert_eq!(format.extension(), ".svg");
	/// ```
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

	/// Attempts to extract a `TileFormat` from the file extension in `filename`.
	///
	/// If a matching extension (e.g. `.pbf` or `.jpeg`) is found, the `TileFormat`
	/// is returned and the filename is truncated to remove the extension.
	/// If no known extension is found, returns `None`.
	///
	/// # Arguments
	///
	/// * `filename` - A mutable `String` representing a filename.  
	///   If an extension is matched, the filename is truncated (the extension removed).
	///
	/// # Examples
	/// ```
	/// use versatiles::types::TileFormat;
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
				".pbf" => TileFormat::PBF,
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
	/// use versatiles::types::TileFormat;
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
			"pbf" => TileFormat::PBF,
			"png" => TileFormat::PNG,
			"svg" => TileFormat::SVG,
			"topojson" => TileFormat::TOPOJSON,
			"webp" => TileFormat::WEBP,
			_ => bail!("Unknown tile format: '{}'", value.trim()),
		})
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
            (TileFormat::PBF, ".pbf"),
            (TileFormat::PNG, ".png"),
            (TileFormat::SVG, ".svg"),
            (TileFormat::TOPOJSON, ".topojson"),
            (TileFormat::WEBP, ".webp"),
        ];

		for (format, expected) in cases {
			assert_eq!(
				format.extension(),
				expected,
				"Expected extension {} for format {:?}",
				expected,
				format
			);
		}
	}

	#[test]
	fn should_extract_correct_format_and_truncate_filename_when_extension_found() {
		struct Case {
			input: &'static str,
			expected_format: Option<TileFormat>,
			expected_filename: &'static str,
		}

		let cases = vec![
			Case {
				input: "image.avif",
				expected_format: Some(TileFormat::AVIF),
				expected_filename: "image",
			},
			Case {
				input: "archive.zip",
				expected_format: None,
				expected_filename: "archive.zip",
			},
			Case {
				input: "binary.bin",
				expected_format: Some(TileFormat::BIN),
				expected_filename: "binary",
			},
			Case {
				input: "noextensionfile",
				expected_format: None,
				expected_filename: "noextensionfile",
			},
			Case {
				input: "unknown.ext",
				expected_format: None,
				expected_filename: "unknown.ext",
			},
			Case {
				input: "data.geojson",
				expected_format: Some(TileFormat::GEOJSON),
				expected_filename: "data",
			},
			Case {
				input: "image.jpeg",
				expected_format: Some(TileFormat::JPG),
				expected_filename: "image",
			},
			Case {
				input: "image.jpg",
				expected_format: Some(TileFormat::JPG),
				expected_filename: "image",
			},
			Case {
				input: "document.json",
				expected_format: Some(TileFormat::JSON),
				expected_filename: "document",
			},
			Case {
				input: "map.pbf",
				expected_format: Some(TileFormat::PBF),
				expected_filename: "map",
			},
			Case {
				input: "picture.png",
				expected_format: Some(TileFormat::PNG),
				expected_filename: "picture",
			},
			Case {
				input: "diagram.svg",
				expected_format: Some(TileFormat::SVG),
				expected_filename: "diagram",
			},
			Case {
				input: "vector.SVG",
				expected_format: Some(TileFormat::SVG),
				expected_filename: "vector",
			},
			Case {
				input: "topography.topojson",
				expected_format: Some(TileFormat::TOPOJSON),
				expected_filename: "topography",
			},
			Case {
				input: "photo.webp",
				expected_format: Some(TileFormat::WEBP),
				expected_filename: "photo",
			},
		];

		for case in cases {
			let mut filename = String::from(case.input);
			let format = TileFormat::from_filename(&mut filename);
			assert_eq!(
				format, case.expected_format,
				"Filename: {}, expected format: {:?}, got: {:?}",
				case.input, case.expected_format, format
			);
			assert_eq!(
				filename, case.expected_filename,
				"Filename after extraction should be '{}' but got '{}'",
				case.expected_filename, filename
			);
		}
	}

	#[test]
	fn should_parse_str_into_tileformat() {
		struct Case {
			input: &'static str,
			expected: Option<TileFormat>,
		}

		let cases = vec![
			Case {
				input: "avif",
				expected: Some(TileFormat::AVIF),
			},
			Case {
				input: ".bin",
				expected: Some(TileFormat::BIN),
			},
			Case {
				input: "GEOJSON",
				expected: Some(TileFormat::GEOJSON),
			},
			Case {
				input: "jpeg",
				expected: Some(TileFormat::JPG),
			},
			Case {
				input: "jpg",
				expected: Some(TileFormat::JPG),
			},
			Case {
				input: ".json",
				expected: Some(TileFormat::JSON),
			},
			Case {
				input: " pbf ",
				expected: Some(TileFormat::PBF),
			},
			Case {
				input: "png",
				expected: Some(TileFormat::PNG),
			},
			Case {
				input: ".topojson",
				expected: Some(TileFormat::TOPOJSON),
			},
			Case {
				input: ".webp",
				expected: Some(TileFormat::WEBP),
			},
			Case {
				input: "unknown",
				expected: None,
			},
		];

		for case in cases {
			let result = TileFormat::parse_str(case.input);
			match case.expected {
				Some(expected_format) => {
					assert_eq!(
						result.unwrap(),
						expected_format,
						"Parsing '{}' should yield {:?}",
						case.input,
						expected_format
					);
				}
				None => {
					assert!(result.is_err(), "Parsing '{}' should fail", case.input);
				}
			}
		}
	}

	#[test]
	fn should_provide_meaningful_strings_for_debug_and_display() {
		let format = TileFormat::PNG;
		assert!(
			format!("{:?}", format).contains("PNG"),
			"Debug output should contain the variant name"
		);
		assert_eq!(
			format!("{}", format),
			"png",
			"Display output should be the lowercase string form"
		);
	}
}
