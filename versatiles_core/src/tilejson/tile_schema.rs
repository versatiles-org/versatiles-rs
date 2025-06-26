use super::tile_content::TileJsonContent;
use anyhow::bail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileJsonSchema {
	RasterRGB,
	RasterRGBA,
	RasterDEMMapbox,
	RasterDEMTerrarium,
	RasterDEMVersatiles,
	VectorOpenMapTiles,
	VectorShortbread1,
	VectorOther,
}

impl TileJsonSchema {
	/// Returns the schema as a string.
	pub fn as_str(&self) -> &str {
		use TileJsonSchema::*;
		match self {
			RasterRGB => "rgb",
			RasterRGBA => "rgba",
			RasterDEMMapbox => "dem/mapbox",
			RasterDEMTerrarium => "dem/terrarium",
			RasterDEMVersatiles => "dem/versatiles",
			VectorOpenMapTiles => "openmaptiles",
			VectorShortbread1 => "shortbread@1.0",
			VectorOther => "other",
		}
	}

	pub fn get_tile_content(&self) -> Option<TileJsonContent> {
		use TileJsonContent::*;
		use TileJsonSchema::*;
		Some(match self {
			RasterRGB | RasterRGBA | RasterDEMMapbox | RasterDEMTerrarium | RasterDEMVersatiles => Raster,
			VectorOpenMapTiles | VectorShortbread1 | VectorOther => Vector,
		})
	}
}

impl std::fmt::Display for TileJsonSchema {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

impl TryFrom<&str> for TileJsonSchema {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		use TileJsonSchema::*;
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
