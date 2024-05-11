use crate::types::TileFormat;
use anyhow::{bail, Result};

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PMTilesType {
	UNKNOWN = 0x0,
	MVT = 0x1,
	PNG = 0x2,
	JPEG = 0x3,
	WEBP = 0x4,
	AVIF = 0x5,
}

impl PMTilesType {
	pub fn from_u8(value: u8) -> Result<Self> {
		match value {
			0 => Ok(PMTilesType::UNKNOWN),
			1 => Ok(PMTilesType::MVT),
			2 => Ok(PMTilesType::PNG),
			3 => Ok(PMTilesType::JPEG),
			4 => Ok(PMTilesType::WEBP),
			5 => Ok(PMTilesType::AVIF),
			_ => bail!("Unknown value {value} for PMTiles type"),
		}
	}
	pub fn from_value(value: TileFormat) -> Result<Self> {
		Ok(match value {
			TileFormat::AVIF => PMTilesType::AVIF,
			TileFormat::BIN => bail!("PMTiles does not support TileFormat::BIN"),
			TileFormat::GEOJSON => bail!("PMTiles does not support TileFormat::GEOJSON"),
			TileFormat::JPG => PMTilesType::JPEG,
			TileFormat::JSON => bail!("PMTiles does not support TileFormat::JSON"),
			TileFormat::PBF => PMTilesType::MVT,
			TileFormat::PNG => PMTilesType::PNG,
			TileFormat::SVG => bail!("PMTiles does not support TileFormat::SVG"),
			TileFormat::TOPOJSON => bail!("PMTiles does not support TileFormat::TOPOJSON"),
			TileFormat::WEBP => PMTilesType::WEBP,
		})
	}
	pub fn as_value(&self) -> Result<TileFormat> {
		Ok(match self {
			PMTilesType::UNKNOWN => TileFormat::BIN,
			PMTilesType::MVT => TileFormat::PBF,
			PMTilesType::PNG => TileFormat::PNG,
			PMTilesType::JPEG => TileFormat::JPG,
			PMTilesType::WEBP => TileFormat::WEBP,
			PMTilesType::AVIF => TileFormat::AVIF,
		})
	}
}