use super::TileType;
use anyhow::bail;
use enumset::{EnumSet, EnumSetType};

/// Known tile schema identifiers.
///
/// A *tile schema* describes how the pixel or vector payload inside a tile
/// is organised and encoded.  Versatiles distinguishes between several
/// raster and vector schemas and falls back to [`Unknown`] if the textual
/// identifier is not recognised.
///
/// ## Raster schemas
/// * **`RasterRGB`** – 3‑band RGB, 8‑bit each.
/// * **`RasterRGBA`** – 4‑band RGBA, 8‑bit each including alpha.
/// * **`RasterDEMMapbox`** – Signed 16‑bit DEM following the *Mapbox*
///   elevation specification.
/// * **`RasterDEMTerrarium`** – Unsigned 16‑bit DEM in the *Terrarium*
///   layout used by GDAL.
/// * **`RasterDEMVersatiles`** – Signed 16‑bit DEM in Versatiles’ own
///   layout.
///
/// ## Vector schemas
/// * **`VectorOpenMapTiles`** – Tiles conform to the *`OpenMapTiles`* MVT
///   schema.
/// * **`VectorShortbread1`** – *Shortbread* schema version 1.0.
/// * **`VectorOther`** – Any other vector schema not listed above.
///
/// ## Unknown
/// * **`Unknown`** – Used when the schema string cannot be parsed.
#[derive(Debug, EnumSetType)]
pub enum TileSchema {
	/// 3-band RGB, 8-bit each.
	RasterRGB,
	/// 4-band RGBA, 8-bit each including alpha.
	RasterRGBA,
	/// Elevation data in Mapbox format (https://docs.mapbox.com/data/tilesets/guides/access-elevation-data/).
	RasterDEMMapbox,
	/// Elevation data in Terrarium format (https://github.com/tilezen/joerd/blob/master/docs/formats.md#terrarium)
	RasterDEMTerrarium,
	/// Elevation data in Versatiles' own format.
	RasterDEMVersatiles,
	/// Vector tiles conforming to the OpenMapTiles schema (https://openmaptiles.org/).
	VectorOpenMapTiles,
	/// Vector tiles conforming to the Shortbread schema (https://shortbread-tiles.org/).
	VectorShortbread1_0,
	/// Any other vector schema not listed above.
	VectorOther,
	/// Used when the schema string cannot be parsed.
	Unknown,
}

impl TileSchema {
	/// Returns the canonical, lower‑case textual identifier of the schema.
	///
	/// The string is suitable for use in URLs, CLI arguments and metadata
	/// files.  The mapping is *loss‑less*: every return value can be parsed
	/// back via `TileSchema::try_from`.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileSchema;
	/// assert_eq!(TileSchema::RasterRGB.as_str(), "rgb");
	/// ```
	#[must_use]
	pub fn as_str(&self) -> &str {
		use TileSchema::*;
		match self {
			RasterRGB => "rgb",
			RasterRGBA => "rgba",
			RasterDEMMapbox => "dem/mapbox",
			RasterDEMTerrarium => "dem/terrarium",
			RasterDEMVersatiles => "dem/versatiles",
			VectorOpenMapTiles => "openmaptiles",
			VectorShortbread1_0 => "shortbread@1.0",
			VectorOther => "other",
			Unknown => "unknown",
		}
	}

	/// Classifies the schema into a broader [`TileType`].
	///
	/// This convenience helper hides the verbose `match` over individual
	/// schemas and lets callers branch on simple content classes (*Raster*,
	/// *Vector* or *Unknown*).
	///
	/// # Examples
	/// ```
	/// use versatiles_core::{TileSchema, TileType};
	/// assert_eq!(TileSchema::RasterRGBA.get_tile_type(), TileType::Raster);
	/// ```
	#[must_use]
	pub fn get_tile_type(&self) -> TileType {
		use TileSchema::*;
		match self {
			RasterRGB | RasterRGBA | RasterDEMMapbox | RasterDEMTerrarium | RasterDEMVersatiles => TileType::Raster,
			VectorOpenMapTiles | VectorShortbread1_0 | VectorOther => TileType::Vector,
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
			"shortbread@1.0" => VectorShortbread1_0,
			"other" => VectorOther,
			_ => bail!(
				"Invalid tile schema: {value}. Only supported schemas are: {}",
				EnumSet::<TileSchema>::all()
					.iter()
					.map(|s| s.as_str().to_string())
					.collect::<Vec<_>>()
					.join(", ")
			),
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
			(VectorShortbread1_0, "shortbread@1.0"),
			(VectorOther, "other"),
			(Unknown, "unknown"),
		] {
			assert_eq!(schema.as_str(), text);
			assert_eq!(format!("{schema}"), text);
		}
	}

	#[test]
	fn test_get_tile_type() {
		use TileSchema::*;
		use TileType::*;

		for (schema, tile_type) in [
			(RasterRGB, Raster),
			(RasterRGBA, Raster),
			(RasterDEMMapbox, Raster),
			(RasterDEMTerrarium, Raster),
			(RasterDEMVersatiles, Raster),
			(VectorOpenMapTiles, Vector),
			(VectorShortbread1_0, Vector),
			(VectorOther, Vector),
			(TileSchema::Unknown, TileType::Unknown),
		] {
			assert_eq!(schema.get_tile_type(), tile_type);
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
			("shortbread@1.0", VectorShortbread1_0),
			("other", VectorOther),
		] {
			assert_eq!(TileSchema::try_from(text).unwrap(), schema);
		}

		assert!(TileSchema::try_from("invalid").is_err());
	}
}
