use anyhow::{bail, Result};

use crate::types::TileCompression;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PMTilesCompression {
	UNKNOWN = 0x0,
	NONE = 0x1,
	GZIP = 0x2,
	BROTLI = 0x3,
	ZSTD = 0x4,
}

impl PMTilesCompression {
	pub fn from_u8(value: u8) -> Result<Self> {
		match value {
			0 => Ok(PMTilesCompression::UNKNOWN),
			1 => Ok(PMTilesCompression::NONE),
			2 => Ok(PMTilesCompression::GZIP),
			3 => Ok(PMTilesCompression::BROTLI),
			4 => Ok(PMTilesCompression::ZSTD),
			_ => bail!("Unknown value {value} for PMTiles compression"),
		}
	}
	pub fn from_value(value: TileCompression) -> Result<Self> {
		Ok(match value {
			TileCompression::None => PMTilesCompression::NONE,
			TileCompression::Gzip => PMTilesCompression::GZIP,
			TileCompression::Brotli => PMTilesCompression::BROTLI,
		})
	}
}
