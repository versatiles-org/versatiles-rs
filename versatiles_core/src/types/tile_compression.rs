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
//! use versatiles_core::types::TileCompression;
//!
//! // Getting file extensions for compression types
//! assert_eq!(TileCompression::Uncompressed.extension(), "");
//! assert_eq!(TileCompression::Gzip.extension(), ".gz");
//! assert_eq!(TileCompression::Brotli.extension(), ".br");
//!
//! // Determining compression type from filename
//! let mut filename = String::from("file.txt.gz");
//! assert_eq!(TileCompression::from_filename(&mut filename), TileCompression::Gzip);
//! assert_eq!(filename, "file.txt");
//! ```

use anyhow::{bail, Result};
#[cfg(feature = "cli")]
use clap::ValueEnum;
use enumset::EnumSetType;
use std::fmt::Display;

/// Enum representing possible compression algorithms.
#[cfg_attr(feature = "cli", derive(ValueEnum))]
#[derive(Debug, EnumSetType, PartialOrd, Ord)]
pub enum TileCompression {
	Uncompressed,
	Gzip,
	Brotli,
}

impl TileCompression {
	pub fn as_str(&self) -> &str {
		match self {
			TileCompression::Uncompressed => "none",
			TileCompression::Gzip => "gzip",
			TileCompression::Brotli => "brotli",
		}
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
	/// use versatiles_core::types::TileCompression;
	///
	/// assert_eq!(TileCompression::Uncompressed.extension(), "");
	/// assert_eq!(TileCompression::Gzip.extension(), ".gz");
	/// assert_eq!(TileCompression::Brotli.extension(), ".br");
	/// ```
	pub fn extension(&self) -> &str {
		match self {
			TileCompression::Uncompressed => "",
			TileCompression::Gzip => ".gz",
			TileCompression::Brotli => ".br",
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
	/// use versatiles_core::types::TileCompression;
	///
	/// let mut filename = String::from("file.txt.gz");
	/// assert_eq!(TileCompression::from_filename(&mut filename), TileCompression::Gzip);
	/// assert_eq!(filename, "file.txt");
	/// ```
	pub fn from_filename(filename: &mut String) -> TileCompression {
		if let Some(index) = filename.rfind('.') {
			let compression = match filename.get(index..).unwrap() {
				".gz" => TileCompression::Gzip,
				".br" => TileCompression::Brotli,
				_ => TileCompression::Uncompressed,
			};

			if compression != TileCompression::Uncompressed {
				filename.truncate(index);
			}
			return compression;
		}
		TileCompression::Uncompressed
	}

	pub fn parse_str(value: &str) -> Result<Self> {
		Ok(match value.to_lowercase().trim() {
			"br" => TileCompression::Brotli,
			"brotli" => TileCompression::Brotli,
			"gz" => TileCompression::Gzip,
			"gzip" => TileCompression::Gzip,
			"none" => TileCompression::Uncompressed,
			"raw" => TileCompression::Uncompressed,
			_ => bail!("Unknown tile compression. Expected brotli, gzip or none"),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_compression_to_extension() {
		fn test(compression: TileCompression, expected_extension: &str) {
			assert_eq!(
				compression.extension(),
				expected_extension,
				"Extension does not match {expected_extension}"
			);
		}

		test(TileCompression::Uncompressed, "");
		test(TileCompression::Gzip, ".gz");
		test(TileCompression::Brotli, ".br");
	}

	#[test]
	fn test_extract_compression() {
		fn test(expected_compression: TileCompression, filename: &str, expected_remainder: &str) {
			let mut filename_string = String::from(filename);
			assert_eq!(
				TileCompression::from_filename(&mut filename_string),
				expected_compression,
				"Extracted compression does not match expected for filename: {filename}"
			);
			assert_eq!(
				filename_string, expected_remainder,
				"Filename remainder does not match expected for filename: {filename}"
			);
		}

		test(TileCompression::Gzip, "file.txt.gz", "file.txt");
		test(TileCompression::Brotli, "archive.tar.br", "archive.tar");
		test(TileCompression::Uncompressed, "image.png", "image.png");
		test(TileCompression::Uncompressed, "document.pdf", "document.pdf");
		test(TileCompression::Uncompressed, "noextensionfile", "noextensionfile");
	}

	#[test]
	fn test_parse_str() {
		fn test(input: &str, expected: Result<TileCompression>) {
			let result = TileCompression::parse_str(input);
			assert_eq!(result.is_ok(), expected.is_ok(), "Unexpected result for input: {input}");
			if let Ok(expected_value) = expected {
				assert_eq!(
					result.unwrap(),
					expected_value,
					"Parsed compression does not match expected for input: {input}"
				);
			} else {
				assert!(result.is_err(), "Expected error for input: {input}");
			}
		}

		test("none", Ok(TileCompression::Uncompressed));
		test("gzip", Ok(TileCompression::Gzip));
		test("brotli", Ok(TileCompression::Brotli));
		test("br", Ok(TileCompression::Brotli));
		test("gz", Ok(TileCompression::Gzip));
		test("raw", Ok(TileCompression::Uncompressed));
		test("unknown", Err(anyhow::anyhow!("Unknown tile compression")));
		test("", Err(anyhow::anyhow!("Unknown tile compression")));
	}

	#[test]
	fn test_display_trait() {
		fn test(compression: TileCompression, expected_display: &str) {
			assert_eq!(
				format!("{}", compression),
				expected_display,
				"Display output does not match expected for {compression:?}"
			);
		}

		test(TileCompression::Uncompressed, "none");
		test(TileCompression::Gzip, "gzip");
		test(TileCompression::Brotli, "brotli");
	}
}
