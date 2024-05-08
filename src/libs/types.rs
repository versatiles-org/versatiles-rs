use clap::ValueEnum;
use versatiles_lib::shared::Compression;

#[derive(Clone, Debug, ValueEnum)]
pub enum TileCompression {
	None,
	Gzip,
	Brotli,
}

impl TileCompression {
	pub fn to_value(&self) -> Compression {
		match self {
			TileCompression::None => Compression::None,
			TileCompression::Gzip => Compression::Gzip,
			TileCompression::Brotli => Compression::Brotli,
		}
	}
}
