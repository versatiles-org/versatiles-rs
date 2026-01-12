//! GeoJSON parser for `versatiles_geometry`.
//!
//! This module parses GeoJSON-like inputs into the crate’s internal types
//! (`GeoCollection`, `GeoFeature`, `Geometry`, `GeoProperties`, `GeoValue`).
//! It uses a streaming `ByteIterator` for zero-allocation-ish parsing with precise
//! error contexts via the `#[context]` macro.

use crate::geo::{GeoCollection, GeoFeature, GeoProperties, GeoValue, Geometry};
use anyhow::{Result, anyhow, bail};
use std::{io::Cursor, str};
use versatiles_core::{
	byte_iterator::{
		ByteIterator, parse_array_entries, parse_number_as, parse_number_as_string, parse_object_entries,
		parse_quoted_json_string, parse_tag,
	},
	json::parse_json_iter,
};
use versatiles_derive::context;

/// Parses a GeoJSON FeatureCollection from a UTF‑8 string into a [`GeoCollection`].
///
/// This is the primary entry point. It creates a byte iterator and delegates to
/// [`parse_geojson_collection`]. The parser is strict and returns detailed errors
/// with context when the input does not conform to GeoJSON expectations.
#[context("parsing GeoJSON root")]
pub fn parse_geojson(json: &str) -> Result<GeoCollection> {
	let mut iter = ByteIterator::from_reader(Cursor::new(json), true);
	parse_geojson_collection(&mut iter)
}

/// Parses a GeoJSON `FeatureCollection` object from the current iterator position.
///
/// Expects an object with `type: "FeatureCollection"` and a `features` array of
/// Feature objects. Unknown members are parsed and ignored.
#[context("parsing GeoJSON FeatureCollection")]
pub fn parse_geojson_collection(iter: &mut ByteIterator) -> Result<GeoCollection> {
	let mut features = Vec::new();
	let mut object_type: Option<String> = None;

	parse_object_entries(iter, |key, iter2| {
		match key.as_str() {
			"type" => object_type = Some(parse_quoted_json_string(iter2)?),
			"features" => features = parse_array_entries(iter2, parse_geojson_feature)?,
			_ => _ = parse_json_iter(iter2)?,
		}
		Ok(())
	})?;

	check_type(object_type, "FeatureCollection")?;

	Ok(GeoCollection { features })
}

/// Validates the required GeoJSON `type` field for a given object.
///
/// Ensures that a `type` is present and matches `name`; otherwise returns an error.
#[context("validating GeoJSON type '{}'", name)]
fn check_type(object_type: Option<String>, name: &str) -> Result<()> {
	let object_type = object_type.ok_or_else(|| anyhow!("{name} must have a type"))?;

	if object_type.as_str() != name {
		bail!("type must be '{name}'")
	}
	Ok(())
}

/// Parses a GeoJSON `Feature` object.
///
/// Reads optional `id`, required `geometry`, and optional `properties`. Unknown
/// members are parsed and ignored. Returns an error if `geometry` is missing.
#[context("parsing GeoJSON Feature")]
pub fn parse_geojson_feature(iter: &mut ByteIterator) -> Result<GeoFeature> {
	let mut object_type: Option<String> = None;
	let mut id: Option<GeoValue> = None;
	let mut geometry: Option<Geometry> = None;
	let mut properties: Option<GeoProperties> = None;

	parse_object_entries(iter, |key, iter2| {
		match key.as_str() {
			"type" => object_type = Some(parse_quoted_json_string(iter2)?),
			"id" => id = Some(parse_geojson_id(iter2)?),
			"geometry" => geometry = Some(parse_geojson_geometry(iter2)?),
			"properties" => properties = Some(parse_geojson_properties(iter2)?),
			_ => _ = parse_json_iter(iter2)?,
		}
		Ok(())
	})?;

	check_type(object_type, "Feature")?;

	Ok(GeoFeature {
		id,
		geometry: geometry.ok_or(anyhow!("feature is missing 'geometry'"))?,
		properties: properties.unwrap_or_default(),
	})
}

/// Parses a GeoJSON `id` field (string or integer). Returns the corresponding [`GeoValue`].
#[context("parsing GeoJSON id field")]
fn parse_geojson_id(iter: &mut ByteIterator) -> Result<GeoValue> {
	iter.skip_whitespace();
	match iter.expect_peeked_byte()? {
		b'"' => parse_quoted_json_string(iter).map(GeoValue::from),
		d if d.is_ascii_digit() => parse_number_as::<u64>(iter).map(GeoValue::UInt),
		c => Err(iter.format_error(&format!(
			"expected a string or integer, but got character '{}'",
			c as char
		))),
	}
}

/// Parses a numeric value and returns a typed [`GeoValue`].
///
/// Detects `f64` if a decimal point is present, `i64` for negative integers,
/// and `u64` for non‑negative integers.
#[context("parsing numeric GeoJSON value")]
fn parse_geojson_number(iter: &mut ByteIterator) -> Result<GeoValue> {
	let number = parse_number_as_string(iter)?;

	Ok(if number.contains('.') {
		GeoValue::from(number.parse::<f64>().map_err(|_| iter.format_error("invalid double"))?)
	} else if number.contains('-') {
		GeoValue::from(
			number
				.parse::<i64>()
				.map_err(|_| iter.format_error("invalid integer"))?,
		)
	} else {
		GeoValue::from(
			number
				.parse::<u64>()
				.map_err(|_| iter.format_error("invalid integer"))?,
		)
	})
}

/// Parses a GeoJSON property value: string, number, boolean, or null.
#[context("parsing GeoJSON property value")]
fn parse_geojson_value(iter: &mut ByteIterator) -> Result<GeoValue> {
	iter.skip_whitespace();
	match iter.expect_peeked_byte()? {
		b'"' => parse_quoted_json_string(iter).map(GeoValue::from),
		d if d.is_ascii_digit() || d == b'.' || d == b'-' => parse_geojson_number(iter),
		b't' => parse_tag(iter, "true").map(|()| GeoValue::Bool(true)),
		b'f' => parse_tag(iter, "false").map(|()| GeoValue::Bool(false)),
		b'n' => parse_tag(iter, "null").map(|()| GeoValue::Null),
		c => Err(iter.format_error(&format!(
			"expected a string or number, but got character '{}'",
			c as char
		))),
	}
}

/// Parses a GeoJSON `properties` object into a [`GeoProperties`] map.
#[context("parsing GeoJSON properties object")]
fn parse_geojson_properties(iter: &mut ByteIterator) -> Result<GeoProperties> {
	let mut list: Vec<(String, GeoValue)> = Vec::new();
	parse_object_entries(iter, |key, iter2| {
		let value = parse_geojson_value(iter2)?;
		list.push((key, value));
		Ok(())
	})?;

	Ok(GeoProperties::from_iter(list))
}

/// Parses a GeoJSON `geometry` object into a [`Geometry`] variant.
///
/// Supports `Point`, `LineString`, `Polygon`, `MultiPoint`, `MultiLineString`, and `MultiPolygon`.
#[context("parsing GeoJSON geometry")]
fn parse_geojson_geometry(iter: &mut ByteIterator) -> Result<Geometry> {
	let mut geometry_type: Option<String> = None;
	let mut coordinates: Option<TemporaryCoordinates> = None;

	parse_object_entries(iter, |key, iter2| {
		match key.as_str() {
			"type" => geometry_type = Some(parse_quoted_json_string(iter2)?),
			"coordinates" => coordinates = Some(parse_geojson_coordinates(iter2)?),
			_ => _ = parse_json_iter(iter2)?,
		}
		Ok(())
	})?;

	let geometry_type = geometry_type.ok_or(anyhow!("geometry must have a type"))?;

	let coordinates = coordinates.ok_or(anyhow!("geometry must have coordinates"))?;
	let geometry = match geometry_type.as_str() {
		"Point" => Geometry::new_point(coordinates.unwrap_c0()),
		"LineString" => Geometry::new_line_string(coordinates.unwrap_c1()),
		"Polygon" => Geometry::new_polygon(coordinates.unwrap_c2()),
		"MultiPoint" => Geometry::new_multi_point(coordinates.unwrap_c1()),
		"MultiLineString" => Geometry::new_multi_line_string(coordinates.unwrap_c2()),
		"MultiPolygon" => Geometry::new_multi_polygon(coordinates.unwrap_c3()),
		_ => bail!("unknown geometry type '{geometry_type}'"),
	};

	Ok(geometry)
}

/// Temporary coordinate accumulator used while recursively parsing nested coordinate arrays.
///
/// This internal enum mirrors the allowed GeoJSON coordinate nesting depths.
enum TemporaryCoordinates {
	V(f64),
	C0([f64; 2]),
	C1(Vec<[f64; 2]>),
	C2(Vec<Vec<[f64; 2]>>),
	C3(Vec<Vec<Vec<[f64; 2]>>>),
}

impl TemporaryCoordinates {
	/// Extracts a single numeric value; panics if the coordinate is not a scalar.
	pub fn unwrap_v(self) -> f64 {
		match self {
			TemporaryCoordinates::V(v) => v,
			_ => panic!("coordinate is not a single value"),
		}
	}
	/// Extracts a single point `[x, y]`; panics if the coordinate is not a point.
	pub fn unwrap_c0(self) -> [f64; 2] {
		match self {
			TemporaryCoordinates::C0(v) => v,
			_ => panic!("coordinates are not a point"),
		}
	}
	/// Extracts an array of points; panics if the coordinate is not a list of points.
	pub fn unwrap_c1(self) -> Vec<[f64; 2]> {
		match self {
			TemporaryCoordinates::C1(v) => v,
			_ => panic!("coordinates are not an array of points"),
		}
	}
	/// Extracts an array of linearly nested point arrays (e.g., rings); panics otherwise.
	pub fn unwrap_c2(self) -> Vec<Vec<[f64; 2]>> {
		match self {
			TemporaryCoordinates::C2(v) => v,
			_ => panic!("coordinates are not an array of an array of points"),
		}
	}
	/// Extracts an array of arrays of point arrays (e.g., polygons); panics otherwise.
	pub fn unwrap_c3(self) -> Vec<Vec<Vec<[f64; 2]>>> {
		match self {
			TemporaryCoordinates::C3(v) => v,
			_ => panic!("coordinates are not an array of an array of an array of points"),
		}
	}
}

/// Recursively parses GeoJSON `coordinates` arrays to the appropriate nesting level.
///
/// Enforces GeoJSON shape constraints (e.g., points are two numbers, no empty arrays,
/// bounded nesting depth for multi‑geometries) and returns a temporary accumulator that
/// is later converted to concrete geometry types.
#[context("parsing GeoJSON coordinate arrays")]
fn parse_geojson_coordinates(iter: &mut ByteIterator) -> Result<TemporaryCoordinates> {
	fn recursive(iter: &mut ByteIterator) -> Result<TemporaryCoordinates> {
		use TemporaryCoordinates::{C0, C1, C2, C3, V};

		iter.skip_whitespace();
		match iter.expect_peeked_byte()? {
			b'[' => {
				let mut list = Vec::new();
				parse_array_entries(iter, |iter2| {
					list.push(recursive(iter2)?);
					Ok(())
				})?;

				if list.is_empty() {
					bail!("empty arrays are not allowed in coordinates")
				}

				let list = match list.first().unwrap() {
					V(_) => {
						if list.len() != 2 {
							bail!("points in coordinates must have exactly two values")
						}
						C0(list
							.into_iter()
							.map(TemporaryCoordinates::unwrap_v)
							.collect::<Vec<f64>>()
							.try_into()
							.unwrap_or_else(|v: Vec<f64>| panic!("Expected a Vec of length {} but it was {}", 2, v.len())))
					}
					C0(_) => C1(list.into_iter().map(TemporaryCoordinates::unwrap_c0).collect()),
					C1(_) => C2(list.into_iter().map(TemporaryCoordinates::unwrap_c1).collect()),
					C2(_) => C3(list.into_iter().map(TemporaryCoordinates::unwrap_c2).collect()),
					C3(_) => bail!("coordinates are nested too deep"),
				};

				Ok(list)
			}
			d if d.is_ascii_digit() || d == b'.' || d == b'-' => parse_number_as(iter).map(V),
			c => Err(iter.format_error(&format!(
				"expected an array or number while parsing coordinates, but got character '{}'",
				c as char
			))),
		}
	}

	recursive(iter)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_geojson_valid_feature_collection() -> Result<()> {
		let json = r#"{
			"type": "FeatureCollection",
			"features": [
				{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{"p":"v"}}
			]
		}"#;

		let collection = parse_geojson(json)?;
		assert_eq!(collection.features.len(), 1);

		let feature = &collection.features[0];
		assert_eq!(feature.geometry.type_name(), "Point");
		if let Geometry::Point(coords) = &feature.geometry {
			assert_eq!(coords.x(), 1.0);
			assert_eq!(coords.y(), 2.0);
		}
		assert_eq!(feature.properties.get("p"), Some(&GeoValue::String("v".to_string())));

		Ok(())
	}

	#[test]
	fn test_parse_geojson_invalid_type() {
		let json = r#"
        {
            "type": "InvalidCollection",
            "features": []
        }
        "#;

		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_missing_geometry() {
		let json = r#"
        {
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "properties": { "prop0": "value0" }
                }
            ]
        }
        "#;

		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_empty_features() -> Result<()> {
		let json = r#"
        {
            "type": "FeatureCollection",
            "features": []
        }
        "#;

		let collection = parse_geojson(json)?;
		assert!(collection.features.is_empty());

		Ok(())
	}

	#[test]
	fn test_parse_geojson_invalid_json() {
		let json = r#"
        {
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [102.0, 0.5] },
                    "properties": { "prop0": "value0" }
                },
            ]
        "#; // Note the trailing comma and unclosed brace

		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_with_id() -> Result<()> {
		let json = r#"
        {
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "id": "feature1",
                    "geometry": { "type": "Point", "coordinates": [102.0, 0.5] },
                    "properties": { "prop0": "value0" }
                }
            ]
        }
        "#;

		let collection = parse_geojson(json)?;
		assert_eq!(collection.features.len(), 1);

		let feature = &collection.features[0];
		assert_eq!(feature.id, Some(GeoValue::String("feature1".to_string())));

		Ok(())
	}

	#[test]
	fn test_parse_geojson_numeric_id() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","id":123,
			"geometry":{"type":"Point","coordinates":[1,2]},"properties":{}
		}]}"#;
		let collection = parse_geojson(json)?;
		assert_eq!(collection.features[0].id, Some(GeoValue::UInt(123)));
		Ok(())
	}

	#[test]
	fn test_parse_geojson_boolean_null_properties() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{"b":true,"n":null}
		}]}"#;
		let collection = parse_geojson(json)?;
		let props = &collection.features[0].properties;
		assert_eq!(props.get("b"), Some(&GeoValue::Bool(true)));
		assert_eq!(props.get("n"), Some(&GeoValue::Null));
		Ok(())
	}

	#[test]
	fn test_parse_geojson_line_string() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0],[1,1]]},"properties":{}
		}]}"#;
		let collection = parse_geojson(json)?;
		assert_eq!(collection.features[0].geometry.type_name(), "LineString");
		Ok(())
	}

	#[test]
	fn test_parse_geojson_polygon() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1],[0,0]]]},"properties":{}
		}]}"#;
		let collection = parse_geojson(json)?;
		assert_eq!(collection.features[0].geometry.type_name(), "Polygon");
		Ok(())
	}

	#[test]
	fn test_parse_geojson_multipoint() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"MultiPoint","coordinates":[[1,2],[3,4]]},"properties":{}
		}]}"#;
		let collection = parse_geojson(json)?;
		assert_eq!(collection.features[0].geometry.type_name(), "MultiPoint");
		Ok(())
	}

	#[test]
	fn test_parse_geojson_multilinestring() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"MultiLineString","coordinates":[[[0,0],[1,1]],[[2,2],[3,3]]]},"properties":{}
		}]}"#;
		let collection = parse_geojson(json)?;
		assert_eq!(collection.features[0].geometry.type_name(), "MultiLineString");
		Ok(())
	}

	#[test]
	fn test_parse_geojson_multipolygon() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,1],[0,0]]]]},"properties":{}
		}]}"#;
		let collection = parse_geojson(json)?;
		assert_eq!(collection.features[0].geometry.type_name(), "MultiPolygon");
		Ok(())
	}

	#[test]
	fn test_parse_geojson_unknown_geometry_type_feature() {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Unknown","coordinates":[0,0]},"properties":{}
		}]}"#;
		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_number_variants() -> Result<()> {
		use std::io::Cursor;
		use versatiles_core::byte_iterator::ByteIterator;
		let cases = vec![
			("123", GeoValue::UInt(123)),
			("-456", GeoValue::Int(-456)),
			("47.11", GeoValue::from(47.11_f64)),
		];
		for (input, expected) in cases {
			let mut iter = ByteIterator::from_reader(Cursor::new(input), true);
			let result = parse_geojson_number(&mut iter)?;
			assert_eq!(result, expected, "input: {input}");
		}
		// Error cases
		for input in &["1.2.3", "abc"] {
			let mut iter = ByteIterator::from_reader(Cursor::new(input), true);
			assert!(parse_geojson_number(&mut iter).is_err(), "{input} should error");
		}
		Ok(())
	}

	#[test]
	fn test_parse_geojson_false_boolean() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{"flag":false}
		}]}"#;
		let collection = parse_geojson(json)?;
		let props = &collection.features[0].properties;
		assert_eq!(props.get("flag"), Some(&GeoValue::Bool(false)));
		Ok(())
	}

	#[test]
	fn test_parse_geojson_negative_number_property() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{"val":-42}
		}]}"#;
		let collection = parse_geojson(json)?;
		let props = &collection.features[0].properties;
		assert_eq!(props.get("val"), Some(&GeoValue::Int(-42)));
		Ok(())
	}

	#[test]
	fn test_parse_geojson_float_property() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{"val":47.11}
		}]}"#;
		let collection = parse_geojson(json)?;
		let props = &collection.features[0].properties;
		assert_eq!(props.get("val"), Some(&GeoValue::Double(47.11)));
		Ok(())
	}

	#[test]
	fn test_parse_geojson_missing_feature_type() {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"geometry":{"type":"Point","coordinates":[0,0]},"properties":{}
		}]}"#;
		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_missing_geometry_type() {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"coordinates":[0,0]},"properties":{}
		}]}"#;
		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_missing_coordinates() {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point"},"properties":{}
		}]}"#;
		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_empty_coordinates_array() {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"LineString","coordinates":[]},"properties":{}
		}]}"#;
		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_wrong_point_dimensions() {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[1,2,3]},"properties":{}
		}]}"#;
		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_single_point_dimension() {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[1]},"properties":{}
		}]}"#;
		let result = parse_geojson(json);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_unknown_members_ignored() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"name":"test",
		"crs":{"type":"name","properties":{}},
		"features":[{
			"type":"Feature",
			"extra":"ignored",
			"geometry":{"type":"Point","coordinates":[1,2],"bbox":[1,2,1,2]},
			"properties":{}
		}]}"#;
		let collection = parse_geojson(json)?;
		assert_eq!(collection.features.len(), 1);
		Ok(())
	}

	#[test]
	fn test_parse_geojson_invalid_id_character() {
		use std::io::Cursor;
		use versatiles_core::byte_iterator::ByteIterator;
		let mut iter = ByteIterator::from_reader(Cursor::new("[1,2]"), true);
		let result = parse_geojson_id(&mut iter);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_invalid_value_character() {
		use std::io::Cursor;
		use versatiles_core::byte_iterator::ByteIterator;
		let mut iter = ByteIterator::from_reader(Cursor::new("[1,2]"), true);
		let result = parse_geojson_value(&mut iter);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_invalid_coordinate_character() {
		use std::io::Cursor;
		use versatiles_core::byte_iterator::ByteIterator;
		let mut iter = ByteIterator::from_reader(Cursor::new("\"invalid\""), true);
		let result = parse_geojson_coordinates(&mut iter);
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_geojson_negative_float_coordinates() -> Result<()> {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[-1.5,-2.5]},"properties":{}
		}]}"#;
		let collection = parse_geojson(json)?;
		if let Geometry::Point(coords) = &collection.features[0].geometry {
			assert_eq!(coords.x(), -1.5);
			assert_eq!(coords.y(), -2.5);
		}
		Ok(())
	}
}
