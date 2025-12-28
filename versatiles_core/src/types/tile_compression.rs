//! This module defines the `TileCompression` enum and associated methods and traits for handling
//! various compression algorithms used in tiles. It includes functionality for converting
//! compression types to file extensions, determining compression from filenames, and displaying
//! compression types as strings.
//!
//! # Features
//!
//! - Supports `None`, `Gzip`, and `Brotli` compression algorithms.
//! - Provides methods for getting file extensions and extracting compression type from filenames.
//!
//! # Examples
//!
//! ```
//! use versatiles_core::TileCompression;
//!
//! // Getting file extensions for compression types
//! assert_eq!(TileCompression::Uncompressed.as_extension(), "");
//! assert_eq!(TileCompression::Gzip.as_extension(), ".gz");
//! assert_eq!(TileCompression::Brotli.as_extension(), ".br");
//!
//! // Determining compression type from filename
//! let mut filename = String::from("file.txt.gz");
//! assert_eq!(TileCompression::from_filename(&mut filename), TileCompression::Gzip);
//! assert_eq!(filename, "file.txt");
//! ```

use TileCompression::*;
use anyhow::{Result, bail};
#[cfg(feature = "cli")]
use clap::ValueEnum;
use enumset::EnumSetType;
use std::fmt::Display;

/// Enum representing possible compression algorithms.
#[cfg_attr(feature = "cli", derive(ValueEnum))]
#[derive(Debug, Default, EnumSetType, PartialOrd, Ord)]
pub enum TileCompression {
	#[default]
	/// No compression.
	Uncompressed,
	/// Gzip compression.
	Gzip,
	/// Brotli compression.
	Brotli,
}

impl TileCompression {
	/// Return the string representation of this compression type.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCompression;
	///
	/// assert_eq!(TileCompression::Gzip.as_str(), "gzip");
	/// assert_eq!(TileCompression::Brotli.as_str(), "brotli");
	/// assert_eq!(TileCompression::Uncompressed.as_str(), "none");
	/// ```
	pub fn as_str(&self) -> &str {
		match self {
			Uncompressed => "none",
			Gzip => "gzip",
			Brotli => "brotli",
		}
	}

	/// Return all possible compression variant strings.
	///
	/// Useful for generating help messages or validating user input.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCompression;
	///
	/// let variants = TileCompression::variants();
	/// assert_eq!(variants, &["none", "gzip", "brotli"]);
	/// ```
	pub fn variants() -> &'static [&'static str] {
		&["none", "gzip", "brotli"]
	}
}

impl Display for TileCompression {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(self.as_str())
	}
}

impl TileCompression {
	/// Returns the file extension associated with the compression type.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCompression;
	///
	/// assert_eq!(TileCompression::Uncompressed.as_extension(), "");
	/// assert_eq!(TileCompression::Gzip.as_extension(), ".gz");
	/// assert_eq!(TileCompression::Brotli.as_extension(), ".br");
	/// ```
	#[must_use]
	pub fn as_extension(&self) -> &str {
		match self {
			Uncompressed => "",
			Gzip => ".gz",
			Brotli => ".br",
		}
	}

	/// Determines the compression type from a given filename.
	///
	/// This method also removes the compression extension from the filename if one is found.
	///
	/// # Arguments
	///
	/// * `filename` - A mutable reference to the filename string.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCompression;
	///
	/// let mut filename = String::from("file.txt.gz");
	/// assert_eq!(TileCompression::from_filename(&mut filename), TileCompression::Gzip);
	/// assert_eq!(filename, "file.txt");
	/// ```
	pub fn from_filename(filename: &mut String) -> TileCompression {
		if let Some(index) = filename.rfind('.') {
			let compression = match filename.get(index..).unwrap() {
				".gz" => Gzip,
				".br" => Brotli,
				_ => Uncompressed,
			};

			if compression != Uncompressed {
				filename.truncate(index);
			}
			return compression;
		}
		Uncompressed
	}
}

impl TryFrom<&str> for TileCompression {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self> {
		Ok(match value.to_lowercase().trim() {
			"br" => Brotli,
			"brotli" => Brotli,
			"gz" => Gzip,
			"gzip" => Gzip,
			"none" => Uncompressed,
			"raw" => Uncompressed,
			_ => bail!("Unknown tile compression. Expected brotli, gzip or none"),
		})
	}
}

impl TryFrom<u8> for TileCompression {
	type Error = anyhow::Error;

	fn try_from(value: u8) -> Result<Self> {
		Ok(match value {
			0 => Uncompressed,
			1 => Gzip,
			2 => Brotli,
			_ => bail!("Unknown tile compression"),
		})
	}
}

impl From<TileCompression> for u8 {
	fn from(compression: TileCompression) -> u8 {
		match compression {
			Uncompressed => 0,
			Gzip => 1,
			Brotli => 2,
		}
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
		for format in EnumSet::<TileCompression>::all() {
			let byte: u8 = format.into();
			let parsed = TileCompression::try_from(byte).unwrap();
			assert_eq!(format, parsed);
			all_bytes.remove(&byte);
		}

		for byte in all_bytes {
			assert!(TileCompression::try_from(byte).is_err());
		}
	}

	#[rstest]
	#[case(Uncompressed, "")]
	#[case(Gzip, ".gz")]
	#[case(Brotli, ".br")]
	fn test_compression_to_extension(#[case] compression: TileCompression, #[case] expected_extension: &str) {
		assert_eq!(compression.as_extension(), expected_extension);
	}

	#[rstest]
	#[case(Gzip, "file.txt.gz", "file.txt")]
	#[case(Brotli, "archive.tar.br", "archive.tar")]
	#[case(Uncompressed, "image.png", "image.png")]
	#[case(Uncompressed, "document.pdf", "document.pdf")]
	#[case(Uncompressed, "noextensionfile", "noextensionfile")]
	fn test_extract_compression(
		#[case] expected_compression: TileCompression,
		#[case] filename: &str,
		#[case] expected_remainder: &str,
	) {
		let mut filename_string = String::from(filename);
		assert_eq!(
			TileCompression::from_filename(&mut filename_string),
			expected_compression,
		);
		assert_eq!(filename_string, expected_remainder);
	}

	#[rstest]
	#[case("none", Ok(TileCompression::Uncompressed))]
	#[case("gzip", Ok(TileCompression::Gzip))]
	#[case("brotli", Ok(TileCompression::Brotli))]
	#[case("br", Ok(TileCompression::Brotli))]
	#[case("gz", Ok(TileCompression::Gzip))]
	#[case("raw", Ok(TileCompression::Uncompressed))]
	#[case("unknown", Err(anyhow::anyhow!("Unknown tile compression")))]
	#[case("", Err(anyhow::anyhow!("Unknown tile compression")))]
	fn test_parse_str(#[case] input: &str, #[case] expected: Result<TileCompression>) {
		let result = TileCompression::try_from(input);
		assert_eq!(result.is_ok(), expected.is_ok());
		if let Ok(expected_value) = expected {
			assert_eq!(result.unwrap(), expected_value);
		} else {
			assert!(result.is_err());
		}
	}

	#[rstest]
	#[case(Uncompressed, "none")]
	#[case(Gzip, "gzip")]
	#[case(Brotli, "brotli")]
	fn test_display_trait(#[case] compression: TileCompression, #[case] expected_display: &str) {
		assert_eq!(format!("{compression}"), expected_display);
	}
}
