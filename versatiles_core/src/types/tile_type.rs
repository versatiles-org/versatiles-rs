use anyhow::bail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileType {
	Raster,
	Vector,
	Unknown,
}

impl TileType {
	pub fn as_str(&self) -> &str {
		use TileType::*;
		match self {
			Raster => "raster",
			Vector => "vector",
			Unknown => "unknown",
		}
	}

	pub fn get_default_tile_schema(&self) -> Option<&'static str> {
		use TileType::*;
		match self {
			Raster => Some("rgb"),
			Vector => Some("other"),
			Unknown => None,
		}
	}
}

impl std::fmt::Display for TileType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

impl TryFrom<&str> for TileType {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		match value {
			"image" | "raster" => Ok(TileType::Raster),
			"vector" => Ok(TileType::Vector),
			"unknown" => Ok(TileType::Unknown),
			_ => bail!("Invalid tile content type: {}", value),
		}
	}
}
