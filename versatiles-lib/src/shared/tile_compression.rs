use enumset::EnumSetType;

/// Enum representing possible compression algorithms
#[derive(Debug, EnumSetType, PartialOrd)]
pub enum Compression {
	None,
	Gzip,
	Brotli,
}

pub fn compression_to_extension(compression: &Compression) -> String {
	String::from(match compression {
		Compression::None => "",
		Compression::Gzip => ".gz",
		Compression::Brotli => ".br",
	})
}

pub fn extract_compression(filename: &mut String) -> Compression {
	if let Some(index) = filename.rfind('.') {
		let compression = match filename.get(index..).unwrap() {
			".gz" => Compression::Gzip,
			".br" => Compression::Brotli,
			_ => Compression::None,
		};

		if compression != Compression::None {
			filename.truncate(index)
		}
		return compression;
	}
	Compression::None
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_compression_to_extension() {
		fn test(compression: Compression, expected_extension: &str) {
			assert_eq!(
				compression_to_extension(&compression),
				expected_extension,
				"Extension does not match {expected_extension}"
			);
		}

		test(Compression::None, "");
		test(Compression::Gzip, ".gz");
		test(Compression::Brotli, ".br");
	}

	#[test]
	fn test_extract_compression() {
		fn test(expected_compression: Compression, filename: &str, expected_remainder: &str) {
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

		test(Compression::Gzip, "file.txt.gz", "file.txt");
		test(Compression::Brotli, "archive.tar.br", "archive.tar");
		test(Compression::None, "image.png", "image.png");
		test(Compression::None, "document.pdf", "document.pdf");
		test(Compression::None, "noextensionfile", "noextensionfile");
	}
}
