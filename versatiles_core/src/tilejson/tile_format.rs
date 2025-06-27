use super::tile_content::TileJsonContent;
use crate::types::TileFormat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileJsonFormat(TileFormat);

impl TileJsonFormat {
	/// Returns the format as a string.
	pub fn as_str(&self) -> &str {
		self.0.as_mime_str()
	}

	pub fn get_tile_content(&self) -> Option<TileJsonContent> {
		use TileFormat::*;
		use TileJsonContent::*;
		match self.0 {
			AVIF | BIN | PNG | JPG | WEBP => Some(Raster),
			GEOJSON | MVT | SVG | TOPOJSON => Some(Vector),
			JSON => None,
		}
	}
}

impl std::fmt::Display for TileJsonFormat {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

impl From<TileFormat> for TileJsonFormat {
	fn from(format: TileFormat) -> Self {
		TileJsonFormat(format)
	}
}

impl TryFrom<&str> for TileJsonFormat {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		TileFormat::try_from_mime(value)
			.or_else(|_| TileFormat::try_from_str(value))
			.map(TileJsonFormat)
			.map_err(|e| anyhow::anyhow!("Invalid tile format: {e}"))
	}
}
