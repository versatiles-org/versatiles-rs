use clap::ValueEnum;
use enumset::EnumSetType;

/// Enum representing possible compression algorithms
#[derive(Debug, EnumSetType)]
#[cfg_attr(feature = "full", derive(ValueEnum))]
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
	if let Some(index) = filename.rfind(".") {
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

