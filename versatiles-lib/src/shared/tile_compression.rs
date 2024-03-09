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
