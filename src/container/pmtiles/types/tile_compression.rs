use anyhow::{bail, Result};

use crate::types::TileCompression;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PMTilesCompression {
	Unknown = 0x0,
	None = 0x1,
	Gzip = 0x2,
	Brotli = 0x3,
	Zstd = 0x4,
}

impl PMTilesCompression {
	pub fn from_u8(value: u8) -> Result<Self> {
		match value {
			0 => Ok(PMTilesCompression::Unknown),
			1 => Ok(PMTilesCompression::None),
			2 => Ok(PMTilesCompression::Gzip),
			3 => Ok(PMTilesCompression::Brotli),
			4 => Ok(PMTilesCompression::Zstd),
			_ => bail!("Unknown value {value} for PMTiles compression"),
		}
	}
	pub fn from_value(value: TileCompression) -> Result<Self> {
		Ok(match value {
			TileCompression::None => PMTilesCompression::None,
			TileCompression::Gzip => PMTilesCompression::Gzip,
			TileCompression::Brotli => PMTilesCompression::Brotli,
		})
	}
	pub fn as_value(&self) -> Result<TileCompression> {
		Ok(match self {
			PMTilesCompression::Unknown => bail!("unknown compression"),
			PMTilesCompression::None => TileCompression::None,
			PMTilesCompression::Gzip => TileCompression::Gzip,
			PMTilesCompression::Brotli => TileCompression::Brotli,
			PMTilesCompression::Zstd => bail!("Zstd not supported yet"),
		})
	}
}
