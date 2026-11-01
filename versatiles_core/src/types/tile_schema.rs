use anyhow::bail;

use super::TileType;

/// Tile schema identifiers.
///
/// A *tile schema* describes how the pixel or vector payload inside a tile
/// is organised and encoded.  VersaTiles distinguishes between several
/// raster and vector schemas, and keeps any other well-formed identifier as
/// [`TileSchema::Custom`], so that a dataset can declare a schema this version
/// has never heard of.
///
/// ## Raster schemas
/// * **`RasterRGB`** – 3‑band RGB, 8‑bit each.
/// * **`RasterRGBA`** – 4‑band RGBA, 8‑bit each including alpha.
/// * **`RasterDEMMapbox`** – Signed 16‑bit DEM following the *Mapbox*
///   elevation specification.
/// * **`RasterDEMTerrarium`** – Unsigned 16‑bit DEM in the *Terrarium*
///   layout used by GDAL.
/// * **`RasterDEMVersatiles`** – Signed 16‑bit DEM in VersaTiles’ own
///   layout.
///
/// ## Vector schemas
/// * **`VectorOpenMapTiles`** – Tiles conform to the *`OpenMapTiles`* MVT
///   schema.
/// * **`VectorShortbread1_0`** – *Shortbread* schema version 1.0.
/// * **`VectorShortbread1_1`** – *Shortbread* schema version 1.1.
/// * **`VectorOther`** – Any other vector schema not listed above.
///
/// ## Others
/// * **`Custom`** – A well-formed identifier that is none of the above.
/// * **`Unknown`** – Used when nothing can be said about the tiles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TileSchema {
	/// 3-band RGB, 8-bit each.
	RasterRGB,
	/// 4-band RGBA, 8-bit each including alpha.
	RasterRGBA,
	/// Elevation data in Mapbox format (<https://docs.mapbox.com/data/tilesets/guides/access-elevation-data/>).
	RasterDEMMapbox,
	/// Elevation data in Terrarium format (<https://github.com/tilezen/joerd/blob/master/docs/formats.md#terrarium>)
	RasterDEMTerrarium,
	/// Elevation data in VersaTiles' own format.
	RasterDEMVersatiles,
	/// Vector tiles conforming to the OpenMapTiles schema (<https://openmaptiles.org/>).
	VectorOpenMapTiles,
	/// Vector tiles conforming to the Shortbread 1.0 schema (<https://shortbread-tiles.org/schema/1.0/>).
	VectorShortbread1_0,
	/// Vector tiles conforming to the Shortbread 1.1 schema (<https://shortbread-tiles.org/schema/1.1/>).
	VectorShortbread1_1,
	/// Any other vector schema not listed above.
	VectorOther,
	/// A well-formed identifier this version does not know, kept verbatim.
	///
	/// Only [`TileSchema::try_from`] should build one, which holds it to the
	/// `<family>[/<subtype>][@<version>]` pattern and never gives a known
	/// identifier this variant.
	Custom(String),
	/// Used when nothing can be said about the tiles, such as for tiles of an
	/// unknown type.
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
		use TileSchema::{
			Custom, RasterDEMMapbox, RasterDEMTerrarium, RasterDEMVersatiles, RasterRGB, RasterRGBA, Unknown,
			VectorOpenMapTiles, VectorOther, VectorShortbread1_0, VectorShortbread1_1,
		};
		match self {
			RasterRGB => "rgb",
			RasterRGBA => "rgba",
			RasterDEMMapbox => "dem/mapbox",
			RasterDEMTerrarium => "dem/terrarium",
			RasterDEMVersatiles => "dem/versatiles",
			VectorOpenMapTiles => "openmaptiles",
			VectorShortbread1_0 => "shortbread@1.0",
			VectorShortbread1_1 => "shortbread@1.1",
			VectorOther => "other",
			Custom(identifier) => identifier,
			Unknown => "unknown",
		}
	}

	/// Return the canonical strings of all schemas this version knows.
	///
	/// This is what a form should offer, not everything [`Self::try_from`]
	/// accepts: that also takes any well-formed identifier. `Unknown` is
	/// intentionally excluded, since it says nothing about the tiles and is
	/// not a value users select.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileSchema;
	/// let v = TileSchema::variants();
	/// assert!(v.contains(&"openmaptiles"));
	/// ```
	#[must_use]
	pub fn variants() -> &'static [&'static str] {
		&[
			"rgb",
			"rgba",
			"dem/mapbox",
			"dem/terrarium",
			"dem/versatiles",
			"openmaptiles",
			"shortbread@1.0",
			"shortbread@1.1",
			"other",
		]
	}

	/// Classifies the schema into a broader [`TileType`].
	///
	/// This convenience helper hides the verbose `match` over individual
	/// schemas and lets callers branch on simple content classes (*Raster*,
	/// *Vector* or *Unknown*).
	///
	/// A [`Custom`](TileSchema::Custom) schema is *Raster* if it is an elevation
	/// schema and *Unknown* otherwise, which says nothing about its tiles; see
	/// [`Self::fits`].
	///
	/// # Examples
	/// ```
	/// use versatiles_core::{TileSchema, TileType};
	/// assert_eq!(TileSchema::RasterRGBA.tile_type(), TileType::Raster);
	/// ```
	#[must_use]
	pub fn tile_type(&self) -> TileType {
		use TileSchema::{
			Custom, RasterDEMMapbox, RasterDEMTerrarium, RasterDEMVersatiles, RasterRGB, RasterRGBA, Unknown,
			VectorOpenMapTiles, VectorOther, VectorShortbread1_0, VectorShortbread1_1,
		};
		match self {
			RasterRGB | RasterRGBA | RasterDEMMapbox | RasterDEMTerrarium | RasterDEMVersatiles => TileType::Raster,
			VectorOpenMapTiles | VectorShortbread1_0 | VectorShortbread1_1 | VectorOther => TileType::Vector,
			Custom(_) if self.is_elevation() => TileType::Raster,
			Custom(_) | Unknown => TileType::Unknown,
		}
	}

	/// Whether this schema can describe tiles of `tile_type`.
	///
	/// A [`Custom`](TileSchema::Custom) schema that is not for elevation can
	/// describe any, since nothing is known about it.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::{TileSchema, TileType};
	/// assert!(!TileSchema::RasterRGB.fits(TileType::Vector));
	///
	/// let custom = TileSchema::try_from("acme/roads@2").unwrap();
	/// assert!(custom.fits(TileType::Vector));
	///
	/// let elevation = TileSchema::try_from("dem/acme").unwrap();
	/// assert!(!elevation.fits(TileType::Vector));
	/// ```
	#[must_use]
	pub fn fits(&self, tile_type: TileType) -> bool {
		(matches!(self, TileSchema::Custom(_)) && !self.is_elevation()) || self.tile_type() == tile_type
	}

	/// Whether the tiles hold elevation data: heights stored in the color
	/// channels of an image, which a lossy encoder would change. That goes for
	/// a [`Custom`](TileSchema::Custom) schema in the `dem` family too.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileSchema;
	/// assert!(TileSchema::RasterDEMMapbox.is_elevation());
	/// assert!(TileSchema::try_from("dem/acme").unwrap().is_elevation());
	/// assert!(!TileSchema::RasterRGB.is_elevation());
	/// ```
	#[must_use]
	pub fn is_elevation(&self) -> bool {
		match self {
			TileSchema::RasterDEMMapbox | TileSchema::RasterDEMTerrarium | TileSchema::RasterDEMVersatiles => true,
			TileSchema::Custom(identifier) => identifier.starts_with("dem/"),
			_ => false,
		}
	}

	/// Whether this version knows the schema, rather than only keeping its name.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileSchema;
	/// assert!(TileSchema::try_from("shortbread@1.1").unwrap().is_known());
	/// assert!(!TileSchema::try_from("shortbraed@1.1").unwrap().is_known());
	/// ```
	#[must_use]
	pub fn is_known(&self) -> bool {
		!matches!(self, TileSchema::Custom(_))
	}
}

/// Whether `value` follows `<family>[/<subtype>][@<version>]`, with each part
/// made of lowercase letters, digits, `.`, `_` and `-`.
fn is_well_formed(value: &str) -> bool {
	let is_part = |part: &str| {
		!part.is_empty()
			&& part
				.bytes()
				.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
	};
	let (name, version) = value.split_once('@').map_or((value, None), |(n, v)| (n, Some(v)));
	let (family, subtype) = name.split_once('/').map_or((name, None), |(f, s)| (f, Some(s)));
	is_part(family) && subtype.is_none_or(is_part) && version.is_none_or(is_part)
}

impl std::fmt::Display for TileSchema {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.as_str())
	}
}

impl TryFrom<&str> for TileSchema {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		use TileSchema::{
			RasterDEMMapbox, RasterDEMTerrarium, RasterDEMVersatiles, RasterRGB, RasterRGBA, Unknown, VectorOpenMapTiles,
			VectorOther, VectorShortbread1_0, VectorShortbread1_1,
		};
		Ok(match value.to_lowercase().as_str() {
			"rgb" => RasterRGB,
			"rgba" => RasterRGBA,
			"dem/mapbox" => RasterDEMMapbox,
			"dem/terrarium" => RasterDEMTerrarium,
			"dem/versatiles" => RasterDEMVersatiles,
			"openmaptiles" => VectorOpenMapTiles,
			"shortbread@1.0" => VectorShortbread1_0,
			"shortbread@1.1" => VectorShortbread1_1,
			"other" => VectorOther,
			// What `as_str` writes for `Unknown`, so a schema that was written
			// out reads back; not offered in `variants`, nor in the list below.
			"unknown" => Unknown,
			_ if is_well_formed(value) => TileSchema::Custom(value.to_string()),
			_ => bail!(
				"Invalid tile schema: {value}. It must be lowercase and follow <family>[/<subtype>][@<version>]. Known schemas are: {}",
				Self::variants().join(", ")
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
			(VectorShortbread1_1, "shortbread@1.1"),
			(VectorOther, "other"),
			(Unknown, "unknown"),
		] {
			assert_eq!(schema.as_str(), text);
			assert_eq!(format!("{schema}"), text);
		}
	}

	#[test]
	fn test_tile_type() {
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
			(VectorShortbread1_1, Vector),
			(VectorOther, Vector),
			(TileSchema::Unknown, TileType::Unknown),
		] {
			assert_eq!(schema.tile_type(), tile_type);
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
			("shortbread@1.1", VectorShortbread1_1),
			("other", VectorOther),
		] {
			assert_eq!(TileSchema::try_from(text).unwrap(), schema);
		}

		assert!(TileSchema::try_from("not a schema").is_err());
	}

	/// `as_str` is documented as loss-less: every schema reads back, `Unknown`
	/// included, which used to be written as "unknown" and then refused — by
	/// an error that listed "unknown" among the supported schemas.
	#[test]
	fn every_schema_round_trips() {
		for schema in [
			TileSchema::RasterRGB,
			TileSchema::RasterRGBA,
			TileSchema::RasterDEMMapbox,
			TileSchema::RasterDEMTerrarium,
			TileSchema::RasterDEMVersatiles,
			TileSchema::VectorOpenMapTiles,
			TileSchema::VectorShortbread1_0,
			TileSchema::VectorShortbread1_1,
			TileSchema::VectorOther,
			TileSchema::Custom("versatiles/landcover@1.0".to_string()),
			TileSchema::Unknown,
		] {
			assert_eq!(TileSchema::try_from(schema.as_str()).unwrap(), schema);
		}
		let error = TileSchema::try_from("not a schema").unwrap_err().to_string();
		assert!(!error.contains("unknown"), "{error}");
	}

	#[test]
	fn a_well_formed_identifier_is_kept_verbatim() {
		for text in [
			"acme",
			"acme@2",
			"acme/roads",
			"versatiles/landcover@1.0",
			"shortbread@9.9",
			"dem/future",
			"my-org_1/a.b@1.0-rc.1",
		] {
			let schema = TileSchema::try_from(text).unwrap();
			assert_eq!(schema, TileSchema::Custom(text.to_string()));
			assert_eq!(schema.as_str(), text);
			assert_eq!(schema.to_string(), text);
			assert!(!schema.is_known(), "{text}");
		}
	}

	#[test]
	fn a_malformed_identifier_is_refused() {
		for text in [
			"",
			" ",
			"not a schema",
			"Acme",
			"acme/Roads",
			"acme@V1",
			"/roads",
			"acme/",
			"@1",
			"acme@",
			"a/b/c",
			"a@1@2",
			"a@1/b",
			"acme\n",
		] {
			assert!(TileSchema::try_from(text).is_err(), "{text:?}");
		}
	}

	/// Spelling is forgiven for a schema we know, and only for one we know: a
	/// `Custom` value is written back as it came, so it has to be right already.
	#[test]
	fn known_names_match_in_any_case_and_become_canonical() {
		for text in ["SHORTBREAD@1.1", "Shortbread@1.1"] {
			assert_eq!(TileSchema::try_from(text).unwrap(), TileSchema::VectorShortbread1_1);
		}
		assert!(TileSchema::try_from("Shortbread@9.9").is_err());
	}

	#[test]
	fn only_the_dem_family_of_custom_schemas_is_elevation() {
		for (text, elevation) in [
			("dem/future", true),
			("dem/future@2", true),
			("dem", false),
			("dem@2", false),
			("demo/roads", false),
			("acme/dem", false),
		] {
			assert_eq!(TileSchema::try_from(text).unwrap().is_elevation(), elevation, "{text}");
		}
	}

	#[test]
	fn a_custom_schema_fits_any_tiles_unless_it_is_elevation() {
		let custom = TileSchema::try_from("acme/roads").unwrap();
		for tile_type in [TileType::Raster, TileType::Vector, TileType::Unknown] {
			assert!(custom.fits(tile_type), "{tile_type:?}");
		}

		let elevation = TileSchema::try_from("dem/future").unwrap();
		assert!(elevation.fits(TileType::Raster));
		assert!(!elevation.fits(TileType::Vector));
	}

	#[test]
	fn a_known_schema_fits_only_its_own_tile_type() {
		assert!(TileSchema::RasterRGB.fits(TileType::Raster));
		assert!(!TileSchema::RasterRGB.fits(TileType::Vector));
		assert!(TileSchema::VectorOther.fits(TileType::Vector));
		assert!(!TileSchema::VectorOther.fits(TileType::Raster));
		assert!(!TileSchema::Unknown.fits(TileType::Raster));
	}

	#[test]
	fn variants_match_try_from() {
		// versatiles_node's codegen builds TS string-literal unions from
		// `variants()`; every value must round-trip through `try_from`.
		for &v in TileSchema::variants() {
			assert!(TileSchema::try_from(v).is_ok(), "{v} should parse");
		}
	}
}
