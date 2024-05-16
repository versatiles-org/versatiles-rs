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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_u8() {
		assert_eq!(PMTilesType::from_u8(0).unwrap(), PMTilesType::UNKNOWN);
		assert_eq!(PMTilesType::from_u8(1).unwrap(), PMTilesType::MVT);
		assert_eq!(PMTilesType::from_u8(2).unwrap(), PMTilesType::PNG);
		assert_eq!(PMTilesType::from_u8(3).unwrap(), PMTilesType::JPEG);
		assert_eq!(PMTilesType::from_u8(4).unwrap(), PMTilesType::WEBP);
		assert_eq!(PMTilesType::from_u8(5).unwrap(), PMTilesType::AVIF);
		assert!(PMTilesType::from_u8(6).is_err());
	}

	#[test]
	fn test_from_value() {
		assert_eq!(PMTilesType::from_value(TileFormat::AVIF).unwrap(), PMTilesType::AVIF);
		assert_eq!(PMTilesType::from_value(TileFormat::JPG).unwrap(), PMTilesType::JPEG);
		assert_eq!(PMTilesType::from_value(TileFormat::PBF).unwrap(), PMTilesType::MVT);
		assert_eq!(PMTilesType::from_value(TileFormat::PNG).unwrap(), PMTilesType::PNG);
		assert_eq!(PMTilesType::from_value(TileFormat::WEBP).unwrap(), PMTilesType::WEBP);

		// Test unsupported TileFormats
		assert!(PMTilesType::from_value(TileFormat::BIN).is_err());
		assert!(PMTilesType::from_value(TileFormat::GEOJSON).is_err());
		assert!(PMTilesType::from_value(TileFormat::JSON).is_err());
		assert!(PMTilesType::from_value(TileFormat::SVG).is_err());
		assert!(PMTilesType::from_value(TileFormat::TOPOJSON).is_err());
	}

	#[test]
	fn test_as_value() {
		assert_eq!(PMTilesType::UNKNOWN.as_value().unwrap(), TileFormat::BIN);
		assert_eq!(PMTilesType::MVT.as_value().unwrap(), TileFormat::PBF);
		assert_eq!(PMTilesType::PNG.as_value().unwrap(), TileFormat::PNG);
		assert_eq!(PMTilesType::JPEG.as_value().unwrap(), TileFormat::JPG);
		assert_eq!(PMTilesType::WEBP.as_value().unwrap(), TileFormat::WEBP);
		assert_eq!(PMTilesType::AVIF.as_value().unwrap(), TileFormat::AVIF);
	}
}
