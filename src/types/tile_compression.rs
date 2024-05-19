use std::fmt::Display;

#[cfg(feature = "cli")]
use clap::ValueEnum;
use enumset::EnumSetType;

/// Enum representing possible compression algorithms
#[cfg_attr(feature = "cli", derive(ValueEnum))]
#[derive(Debug, EnumSetType, PartialOrd)]
pub enum TileCompression {
	None,
	Gzip,
	Brotli,
}

impl Display for TileCompression {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(match self {
			TileCompression::None => "none",
			TileCompression::Gzip => "gzip",
			TileCompression::Brotli => "brotli",
		})
	}
}

pub fn compression_to_extension(compression: &TileCompression) -> String {
	String::from(match compression {
		TileCompression::None => "",
		TileCompression::Gzip => ".gz",
		TileCompression::Brotli => ".br",
	})
}

pub fn extract_compression(filename: &mut String) -> TileCompression {
	if let Some(index) = filename.rfind('.') {
		let compression = match filename.get(index..).unwrap() {
			".gz" => TileCompression::Gzip,
			".br" => TileCompression::Brotli,
			_ => TileCompression::None,
		};

		if compression != TileCompression::None {
			filename.truncate(index)
		}
		return compression;
	}
	TileCompression::None
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_compression_to_extension() {
		fn test(compression: TileCompression, expected_extension: &str) {
			assert_eq!(
				compression_to_extension(&compression),
				expected_extension,
				"Extension does not match {expected_extension}"
			);
		}

		test(TileCompression::None, "");
		test(TileCompression::Gzip, ".gz");
		test(TileCompression::Brotli, ".br");
	}

	#[test]
	fn test_extract_compression() {
		fn test(expected_compression: TileCompression, filename: &str, expected_remainder: &str) {
			let mut filename_string = String::from(filename);
			assert_eq!(
				extract_compression(&mut filename_string),
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
		test(TileCompression::None, "image.png", "image.png");
		test(TileCompression::None, "document.pdf", "document.pdf");
		test(TileCompression::None, "noextensionfile", "noextensionfile");
	}
}
