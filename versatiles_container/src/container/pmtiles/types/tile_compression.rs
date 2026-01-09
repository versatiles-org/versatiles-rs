use anyhow::{Result, bail};
use versatiles_core::TileCompression::{self, Brotli, Gzip, Uncompressed};

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
			Uncompressed => PMTilesCompression::None,
			Gzip => PMTilesCompression::Gzip,
			Brotli => PMTilesCompression::Brotli,
		})
	}
	pub fn as_value(&self) -> Result<TileCompression> {
		Ok(match self {
			PMTilesCompression::Unknown => bail!("unknown compression"),
			PMTilesCompression::None => Uncompressed,
			PMTilesCompression::Gzip => Gzip,
			PMTilesCompression::Brotli => Brotli,
			PMTilesCompression::Zstd => bail!("Zstd not supported yet"),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_u8() {
		assert_eq!(PMTilesCompression::from_u8(0).unwrap(), PMTilesCompression::Unknown);
		assert_eq!(PMTilesCompression::from_u8(1).unwrap(), PMTilesCompression::None);
		assert_eq!(PMTilesCompression::from_u8(2).unwrap(), PMTilesCompression::Gzip);
		assert_eq!(PMTilesCompression::from_u8(3).unwrap(), PMTilesCompression::Brotli);
		assert_eq!(PMTilesCompression::from_u8(4).unwrap(), PMTilesCompression::Zstd);
		assert!(PMTilesCompression::from_u8(5).is_err());
	}

	#[test]
	fn test_from_value() {
		assert_eq!(
			PMTilesCompression::from_value(Uncompressed).unwrap(),
			PMTilesCompression::None
		);
		assert_eq!(PMTilesCompression::from_value(Gzip).unwrap(), PMTilesCompression::Gzip);
		assert_eq!(
			PMTilesCompression::from_value(Brotli).unwrap(),
			PMTilesCompression::Brotli
		);

		// Zstd is not supported in TileCompression, so we don't test it here
	}

	#[test]
	fn test_as_value() {
		assert!(PMTilesCompression::Unknown.as_value().is_err());
		assert_eq!(PMTilesCompression::None.as_value().unwrap(), Uncompressed);
		assert_eq!(PMTilesCompression::Gzip.as_value().unwrap(), Gzip);
		assert_eq!(PMTilesCompression::Brotli.as_value().unwrap(), Brotli);
		assert!(PMTilesCompression::Zstd.as_value().is_err());
	}

	#[test]
	fn test_conversion_cycle() {
		// Test cycle: PMTilesCompression -> u8 -> PMTilesCompression
		let compression = PMTilesCompression::Gzip;
		let value = compression as u8;
		let converted_back = PMTilesCompression::from_u8(value).unwrap();
		assert_eq!(compression, converted_back);

		// Test cycle: TileCompression -> PMTilesCompression -> TileCompression
		let tile_compression = Gzip;
		let pm_compression = PMTilesCompression::from_value(tile_compression).unwrap();
		let converted_back = pm_compression.as_value().unwrap();
		assert_eq!(tile_compression, converted_back);
	}
}
