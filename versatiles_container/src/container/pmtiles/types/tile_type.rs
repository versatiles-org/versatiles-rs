use anyhow::{Result, bail};
use versatiles_core::TileFormat;

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
		use TileFormat::*;
		Ok(match value {
			AVIF => PMTilesType::AVIF,
			BIN => bail!("PMTiles does not support BIN"),
			GEOJSON => bail!("PMTiles does not support GEOJSON"),
			JPG => PMTilesType::JPEG,
			JSON => bail!("PMTiles does not support JSON"),
			MVT => PMTilesType::MVT,
			PNG => PMTilesType::PNG,
			SVG => bail!("PMTiles does not support SVG"),
			TOPOJSON => bail!("PMTiles does not support TOPOJSON"),
			WEBP => PMTilesType::WEBP,
		})
	}
	pub fn as_value(&self) -> Result<TileFormat> {
		use TileFormat::*;
		Ok(match self {
			PMTilesType::UNKNOWN => BIN,
			PMTilesType::MVT => MVT,
			PMTilesType::PNG => PNG,
			PMTilesType::JPEG => JPG,
			PMTilesType::WEBP => WEBP,
			PMTilesType::AVIF => AVIF,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use TileFormat::*;

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
		assert_eq!(PMTilesType::from_value(AVIF).unwrap(), PMTilesType::AVIF);
		assert_eq!(PMTilesType::from_value(JPG).unwrap(), PMTilesType::JPEG);
		assert_eq!(PMTilesType::from_value(MVT).unwrap(), PMTilesType::MVT);
		assert_eq!(PMTilesType::from_value(PNG).unwrap(), PMTilesType::PNG);
		assert_eq!(PMTilesType::from_value(WEBP).unwrap(), PMTilesType::WEBP);

		// Test unsupported TileFormats
		assert!(PMTilesType::from_value(BIN).is_err());
		assert!(PMTilesType::from_value(GEOJSON).is_err());
		assert!(PMTilesType::from_value(JSON).is_err());
		assert!(PMTilesType::from_value(SVG).is_err());
		assert!(PMTilesType::from_value(TOPOJSON).is_err());
	}

	#[test]
	fn test_as_value() {
		assert_eq!(PMTilesType::UNKNOWN.as_value().unwrap(), BIN);
		assert_eq!(PMTilesType::MVT.as_value().unwrap(), MVT);
		assert_eq!(PMTilesType::PNG.as_value().unwrap(), PNG);
		assert_eq!(PMTilesType::JPEG.as_value().unwrap(), JPG);
		assert_eq!(PMTilesType::WEBP.as_value().unwrap(), WEBP);
		assert_eq!(PMTilesType::AVIF.as_value().unwrap(), AVIF);
	}
}
