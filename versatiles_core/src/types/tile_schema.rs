use super::TileType;
use anyhow::bail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileSchema {
	RasterRGB,
	RasterRGBA,
	RasterDEMMapbox,
	RasterDEMTerrarium,
	RasterDEMVersatiles,
	VectorOpenMapTiles,
	VectorShortbread1,
	VectorOther,
	Unknown,
}

impl TileSchema {
	/// Returns the schema as a string.
	pub fn as_str(&self) -> &str {
		use TileSchema::*;
		match self {
			RasterRGB => "rgb",
			RasterRGBA => "rgba",
			RasterDEMMapbox => "dem/mapbox",
			RasterDEMTerrarium => "dem/terrarium",
			RasterDEMVersatiles => "dem/versatiles",
			VectorOpenMapTiles => "openmaptiles",
			VectorShortbread1 => "shortbread@1.0",
			VectorOther => "other",
			Unknown => "unknown",
		}
	}

	pub fn get_tile_content(&self) -> TileType {
		use TileSchema::*;
		match self {
			RasterRGB | RasterRGBA | RasterDEMMapbox | RasterDEMTerrarium | RasterDEMVersatiles => TileType::Raster,
			VectorOpenMapTiles | VectorShortbread1 | VectorOther => TileType::Vector,
			Unknown => TileType::Unknown,
		}
	}
}

impl std::fmt::Display for TileSchema {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

impl TryFrom<&str> for TileSchema {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		use TileSchema::*;
		Ok(match value.to_lowercase().as_str() {
			"rgb" => RasterRGB,
			"rgba" => RasterRGBA,
			"dem/mapbox" => RasterDEMMapbox,
			"dem/terrarium" => RasterDEMTerrarium,
			"dem/versatiles" => RasterDEMVersatiles,
			"openmaptiles" => VectorOpenMapTiles,
			"shortbread@1.0" => VectorShortbread1,
			"other" => VectorOther,
			_ => bail!("Invalid tile schema: {}", value),
		})
	}
}
