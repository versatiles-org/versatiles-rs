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
pub fn from_tile_schema(schema: &Option<TileSchema>) -> Result<DemEncoding> {
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

/// Parse a DEM encoding from a string parameter.
pub fn parse_encoding(s: &str) -> Result<DemEncoding> {
	match s {
		"mapbox" => Ok(DemEncoding::Mapbox),
		"terrarium" => Ok(DemEncoding::Terrarium),
		other => bail!("Unknown DEM encoding '{other}'; expected 'mapbox' or 'terrarium'"),
	}
}

/// Resolve DEM encoding from an optional string override and a tile schema.
///
/// If `encoding_str` is provided, it is parsed directly.
/// Otherwise, the encoding is auto-detected from the tile schema.
pub fn resolve_encoding(encoding_str: &Option<String>, schema: &Option<TileSchema>) -> Result<DemEncoding> {
	if let Some(enc_str) = encoding_str {
		parse_encoding(enc_str)
	} else {
		from_tile_schema(schema)
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
			from_tile_schema(&Some(TileSchema::RasterDEMMapbox)).unwrap(),
			DemEncoding::Mapbox
		);
		assert_eq!(
			from_tile_schema(&Some(TileSchema::RasterDEMTerrarium)).unwrap(),
			DemEncoding::Terrarium
		);
		assert!(from_tile_schema(&None).is_err());
		assert!(from_tile_schema(&Some(TileSchema::RasterRGB)).is_err());
	}

	#[test]
	fn test_to_tile_schema() {
		assert_eq!(to_tile_schema(DemEncoding::Mapbox), TileSchema::RasterDEMMapbox);
		assert_eq!(to_tile_schema(DemEncoding::Terrarium), TileSchema::RasterDEMTerrarium);
	}

	#[test]
	fn test_parse_encoding() {
		assert_eq!(parse_encoding("mapbox").unwrap(), DemEncoding::Mapbox);
		assert_eq!(parse_encoding("terrarium").unwrap(), DemEncoding::Terrarium);
		assert!(parse_encoding("invalid").is_err());
	}

	#[test]
	fn test_resolve_encoding_with_override() {
		let result = resolve_encoding(&Some("terrarium".to_string()), &None);
		assert_eq!(result.unwrap(), DemEncoding::Terrarium);
	}

	#[test]
	fn test_resolve_encoding_from_schema() {
		let result = resolve_encoding(&None, &Some(TileSchema::RasterDEMMapbox));
		assert_eq!(result.unwrap(), DemEncoding::Mapbox);
	}

	#[test]
	fn test_resolve_encoding_override_takes_priority() {
		let result = resolve_encoding(&Some("terrarium".to_string()), &Some(TileSchema::RasterDEMMapbox));
		assert_eq!(result.unwrap(), DemEncoding::Terrarium);
	}
}
