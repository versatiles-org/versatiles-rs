//! GeoJSON parser for `versatiles_geometry`.
//!
//! This module parses GeoJSON-like inputs into the crate’s internal types
//! (`GeoCollection`, `GeoFeature`, `Geometry`, `GeoProperties`, `GeoValue`).
//! It uses a streaming `ByteIterator` for zero-allocation-ish parsing with precise
//! error contexts via the `#[context]` macro.

use crate::geo::{GeoCollection, GeoFeature, GeoProperties, GeoValue};
use anyhow::{Result, anyhow, bail};
use geo_types::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
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
	let mut geometry: Option<Geometry<f64>> = None;
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
fn parse_geojson_geometry(iter: &mut ByteIterator) -> Result<Geometry<f64>> {
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
		"Point" => Geometry::Point(point_from_c0(coordinates.take_c0()?)),
		"LineString" => Geometry::LineString(line_string_from_c1(coordinates.take_c1()?)),
		"Polygon" => Geometry::Polygon(polygon_from_c2(coordinates.take_c2()?)),
		"MultiPoint" => Geometry::MultiPoint(MultiPoint(
			coordinates.take_c1()?.into_iter().map(point_from_c0).collect(),
		)),
		"MultiLineString" => Geometry::MultiLineString(MultiLineString(
			coordinates.take_c2()?.into_iter().map(line_string_from_c1).collect(),
		)),
		"MultiPolygon" => Geometry::MultiPolygon(MultiPolygon(
			coordinates.take_c3()?.into_iter().map(polygon_from_c2).collect(),
		)),
		_ => bail!("unknown geometry type '{geometry_type}'"),
	};

	Ok(geometry)
}

fn coord_from(c: [f64; 2]) -> Coord<f64> {
	Coord { x: c[0], y: c[1] }
}

fn point_from_c0(c: [f64; 2]) -> Point<f64> {
	Point(coord_from(c))
}

fn line_string_from_c1(coords: Vec<[f64; 2]>) -> LineString<f64> {
	LineString::new(coords.into_iter().map(coord_from).collect())
}

fn polygon_from_c2(rings: Vec<Vec<[f64; 2]>>) -> Polygon<f64> {
	let mut iter = rings.into_iter().map(line_string_from_c1);
	let exterior = iter.next().unwrap_or_else(|| LineString::new(vec![]));
	let interiors = iter.collect();
	Polygon::new(exterior, interiors)
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
	/// Extracts a single numeric value.
	pub fn take_v(self) -> Result<f64> {
		match self {
			TemporaryCoordinates::V(v) => Ok(v),
			_ => bail!("coordinate is not a single value"),
		}
	}
	/// Extracts a single point `[x, y]`.
	pub fn take_c0(self) -> Result<[f64; 2]> {
		match self {
			TemporaryCoordinates::C0(v) => Ok(v),
			_ => bail!("coordinates are not a point"),
		}
	}
	/// Extracts an array of points.
	pub fn take_c1(self) -> Result<Vec<[f64; 2]>> {
		match self {
			TemporaryCoordinates::C1(v) => Ok(v),
			_ => bail!("coordinates are not an array of points"),
		}
	}
	/// Extracts an array of linearly nested point arrays (e.g., rings).
	pub fn take_c2(self) -> Result<Vec<Vec<[f64; 2]>>> {
		match self {
			TemporaryCoordinates::C2(v) => Ok(v),
			_ => bail!("coordinates are not an array of an array of points"),
		}
	}
	/// Extracts an array of arrays of point arrays (e.g., polygons).
	pub fn take_c3(self) -> Result<Vec<Vec<Vec<[f64; 2]>>>> {
		match self {
			TemporaryCoordinates::C3(v) => Ok(v),
			_ => bail!("coordinates are not an array of an array of an array of points"),
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

				let list = match list.first().expect("checked non-empty above") {
					V(_) => {
						// RFC 7946: a position is "an array of numbers… two or more
						// elements", with the optional 3rd being altitude. We're 2D-only,
						// so take the first two and ignore the rest. Real-world feeds
						// also stuff `null` into the altitude slot — we tolerate that
						// (parse_geojson_coordinates accepts `null` as NaN below) and
						// drop those slots silently. NaN in the lon/lat slots stays an
						// error.
						if list.len() < 2 {
							bail!("points in coordinates must have at least two values")
						}
						let x = list.remove(0).take_v()?;
						let y = list.remove(0).take_v()?;
						if !x.is_finite() || !y.is_finite() {
							bail!("longitude and latitude must be finite numbers")
						}
						C0([x, y])
					}
					C0(_) => C1(list
						.into_iter()
						.map(TemporaryCoordinates::take_c0)
						.collect::<Result<_>>()?),
					C1(_) => C2(list
						.into_iter()
						.map(TemporaryCoordinates::take_c1)
						.collect::<Result<_>>()?),
					C2(_) => C3(list
						.into_iter()
						.map(TemporaryCoordinates::take_c2)
						.collect::<Result<_>>()?),
					C3(_) => bail!("coordinates are nested too deep"),
				};

				Ok(list)
			}
			d if d.is_ascii_digit() || d == b'.' || d == b'-' => parse_number_as(iter).map(V),
			b'n' => parse_tag(iter, "null").map(|()| V(f64::NAN)),
			c => Err(iter.format_error(&format!(
				"expected an array or number while parsing coordinates, but got character '{}'",
				c as char
			))),
		}
	}

	recursive(iter)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ext::type_name;
	use approx::assert_relative_eq;

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
		assert_eq!(type_name(&feature.geometry), "Point");
		if let Geometry::Point(coords) = &feature.geometry {
			assert_relative_eq!(coords.x(), 1.0);
			assert_relative_eq!(coords.y(), 2.0);
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
		assert_eq!(type_name(&collection.features[0].geometry), "LineString");
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
		assert_eq!(type_name(&collection.features[0].geometry), "Polygon");
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
		assert_eq!(type_name(&collection.features[0].geometry), "MultiPoint");
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
		assert_eq!(type_name(&collection.features[0].geometry), "MultiLineString");
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
		assert_eq!(type_name(&collection.features[0].geometry), "MultiPolygon");
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
	fn test_parse_geojson_three_dimensional_point_drops_altitude() {
		// RFC 7946 §3.1.1 allows an optional 3rd element (altitude). We're
		// 2D-only, so the altitude is silently dropped.
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[1,2,3]},"properties":{}
		}]}"#;
		let collection = parse_geojson(json).expect("3D point should parse");
		match &collection.features[0].geometry {
			Geometry::Point(p) => assert!((p.x() - 1.0).abs() < 1e-9 && (p.y() - 2.0).abs() < 1e-9),
			other => panic!("expected Point, got {other:?}"),
		}
	}

	#[test]
	fn test_parse_geojson_null_altitude_tolerated() {
		// Real-world feeds (e.g. USGS earthquakes) emit `null` as altitude when
		// the value is unknown. Tolerate it.
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[1,2,null]},"properties":{}
		}]}"#;
		parse_geojson(json).expect("null altitude should parse");
	}

	#[test]
	fn test_parse_geojson_null_lon_or_lat_errors() {
		let json = r#"{
		"type":"FeatureCollection",
		"features":[{
			"type":"Feature","geometry":{"type":"Point","coordinates":[null,2]},"properties":{}
		}]}"#;
		assert!(parse_geojson(json).is_err());
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
			assert_relative_eq!(coords.x(), -1.5);
			assert_relative_eq!(coords.y(), -2.5);
		}
		Ok(())
	}
}
