use anyhow::bail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileJsonContent {
	Raster,
	Vector,
}

impl TileJsonContent {
	/// Returns the content type as a string.
	pub fn as_str(&self) -> &str {
		match self {
			TileJsonContent::Raster => "raster",
			TileJsonContent::Vector => "vector",
		}
	}

	pub fn get_default_tile_schema(&self) -> Option<&'static str> {
		use TileJsonContent::*;
		match self {
			Raster => Some("rgb"),
			Vector => Some("other"),
		}
	}
}

impl std::fmt::Display for TileJsonContent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

impl TryFrom<&str> for TileJsonContent {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		match value {
			"image" | "raster" => Ok(TileJsonContent::Raster),
			"vector" => Ok(TileJsonContent::Vector),
			_ => bail!("Invalid tile content type: {}", value),
		}
	}
}
