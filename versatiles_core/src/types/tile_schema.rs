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

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn test_as_str() {
		use TileSchema::*;

		for (schema, text) in [
			(RasterRGB, "rgb"),
			(RasterRGBA, "rgba"),
			(RasterDEMMapbox, "dem/mapbox"),
			(RasterDEMTerrarium, "dem/terrarium"),
			(RasterDEMVersatiles, "dem/versatiles"),
			(VectorOpenMapTiles, "openmaptiles"),
			(VectorShortbread1, "shortbread@1.0"),
			(VectorOther, "other"),
			(Unknown, "unknown"),
		] {
			assert_eq!(schema.as_str(), text);
			assert_eq!(format!("{}", schema), text);
		}
	}

	#[test]
	fn test_get_tile_content() {
		use TileSchema::*;
		use TileType::*;

		for (schema, tile_type) in [
			(RasterRGB, Raster),
			(RasterRGBA, Raster),
			(RasterDEMMapbox, Raster),
			(RasterDEMTerrarium, Raster),
			(RasterDEMVersatiles, Raster),
			(VectorOpenMapTiles, Vector),
			(VectorShortbread1, Vector),
			(VectorOther, Vector),
			(TileSchema::Unknown, TileType::Unknown),
		] {
			assert_eq!(schema.get_tile_content(), tile_type);
		}
	}

	#[test]
	fn test_try_from() {
		use TileSchema::*;

		for (text, schema) in [
			("rgb", RasterRGB),
			("rgba", RasterRGBA),
			("dem/mapbox", RasterDEMMapbox),
			("dem/terrarium", RasterDEMTerrarium),
			("dem/versatiles", RasterDEMVersatiles),
			("openmaptiles", VectorOpenMapTiles),
			("shortbread@1.0", VectorShortbread1),
			("other", VectorOther),
		] {
			assert_eq!(TileSchema::try_from(text).unwrap(), schema);
		}

		assert!(TileSchema::try_from("invalid").is_err());
	}
}
