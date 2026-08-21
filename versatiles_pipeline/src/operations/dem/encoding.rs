use anyhow::{Result, bail};
use versatiles_core::TileSchema;

/// DEM encoding format for elevation data stored as RGB pixel values.
///
/// Both encodings store elevation as a 24-bit integer split across R, G, B channels:
/// `raw = R * 65536 + G * 256 + B`
///
/// The encoding determines the mapping between raw values and elevation in meters:
/// - **Mapbox**: `elevation = raw * 0.1 - 10000`
/// - **Terrarium**: `elevation = raw / 256 - 32768`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemEncoding {
	Mapbox,
	Terrarium,
}

/// Returns the number of meters per raw DEM unit for the given encoding.
pub fn raw_unit_meters(encoding: DemEncoding) -> f64 {
	match encoding {
		DemEncoding::Mapbox => 0.1,            // 1 raw unit = 0.1 m
		DemEncoding::Terrarium => 1.0 / 256.0, // 1 raw unit = 1/256 m
	}
}

/// Detect `DemEncoding` from a `TileSchema`.
pub fn from_tile_schema(schema: Option<&TileSchema>) -> Result<DemEncoding> {
	match schema {
		Some(TileSchema::RasterDEMMapbox) => Ok(DemEncoding::Mapbox),
		Some(TileSchema::RasterDEMTerrarium) => Ok(DemEncoding::Terrarium),
		_ => bail!("tile_schema is not a DEM encoding (mapbox/terrarium); use the 'encoding' parameter to specify one"),
	}
}

/// Convert a `DemEncoding` to the corresponding `TileSchema`.
pub fn to_tile_schema(encoding: DemEncoding) -> TileSchema {
	match encoding {
		DemEncoding::Mapbox => TileSchema::RasterDEMMapbox,
		DemEncoding::Terrarium => TileSchema::RasterDEMTerrarium,
	}
}

impl DemEncoding {
	/// The accepted VPL spellings, in the order they should be listed.
	///
	/// The single source of truth for what `encoding=` accepts: `VPLDecode`
	/// renders these into the generated parameter reference and the TypeScript
	/// bindings, so no doc comment repeats them. Kept in step with
	/// [`TryFrom<&str>`] by `variants_match_try_from`.
	#[must_use]
	pub fn variants() -> &'static [&'static str] {
		&["mapbox", "terrarium"]
	}
}

impl TryFrom<&str> for DemEncoding {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self> {
		match value {
			"mapbox" => Ok(DemEncoding::Mapbox),
			"terrarium" => Ok(DemEncoding::Terrarium),
			other => bail!(
				"Unknown DEM encoding '{other}'; expected one of {}",
				DemEncoding::variants().join(", ")
			),
		}
	}
}

/// Resolve DEM encoding from an optional explicit override and a tile schema.
///
/// The override wins when given; otherwise the encoding is auto-detected from
/// the tile schema.
pub fn resolve_encoding(encoding: Option<DemEncoding>, schema: Option<&TileSchema>) -> Result<DemEncoding> {
	match encoding {
		Some(encoding) => Ok(encoding),
		None => from_tile_schema(schema),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_raw_unit_meters() {
		assert!((raw_unit_meters(DemEncoding::Mapbox) - 0.1).abs() < f64::EPSILON);
		assert!((raw_unit_meters(DemEncoding::Terrarium) - 1.0 / 256.0).abs() < f64::EPSILON);
	}

	#[test]
	fn test_from_tile_schema() {
		assert_eq!(
			from_tile_schema(Some(&TileSchema::RasterDEMMapbox)).unwrap(),
			DemEncoding::Mapbox
		);
		assert_eq!(
			from_tile_schema(Some(&TileSchema::RasterDEMTerrarium)).unwrap(),
			DemEncoding::Terrarium
		);
		assert!(from_tile_schema(None).is_err());
		assert!(from_tile_schema(Some(&TileSchema::RasterRGB)).is_err());
	}

	#[test]
	fn test_to_tile_schema() {
		assert_eq!(to_tile_schema(DemEncoding::Mapbox), TileSchema::RasterDEMMapbox);
		assert_eq!(to_tile_schema(DemEncoding::Terrarium), TileSchema::RasterDEMTerrarium);
	}

	#[test]
	fn test_try_from_str() {
		assert_eq!(DemEncoding::try_from("mapbox").unwrap(), DemEncoding::Mapbox);
		assert_eq!(DemEncoding::try_from("terrarium").unwrap(), DemEncoding::Terrarium);
		assert!(DemEncoding::try_from("invalid").is_err());
	}

	/// Guards the promise `variants()` makes to the generated reference: every
	/// listed spelling parses, and nothing parses that is not listed.
	#[test]
	fn variants_match_try_from() {
		for variant in DemEncoding::variants() {
			assert!(
				DemEncoding::try_from(*variant).is_ok(),
				"variant {variant:?} does not parse"
			);
		}
		assert_eq!(DemEncoding::variants().len(), 2);
	}

	#[test]
	fn test_resolve_encoding_with_override() {
		let result = resolve_encoding(Some(DemEncoding::Terrarium), None);
		assert_eq!(result.unwrap(), DemEncoding::Terrarium);
	}

	#[test]
	fn test_resolve_encoding_from_schema() {
		let schema = TileSchema::RasterDEMMapbox;
		let result = resolve_encoding(None, Some(&schema));
		assert_eq!(result.unwrap(), DemEncoding::Mapbox);
	}

	#[test]
	fn test_resolve_encoding_override_takes_priority() {
		let schema = TileSchema::RasterDEMMapbox;
		let result = resolve_encoding(Some(DemEncoding::Terrarium), Some(&schema));
		assert_eq!(result.unwrap(), DemEncoding::Terrarium);
	}
}
