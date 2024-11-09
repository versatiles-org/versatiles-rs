use crate::{
	utils::{parse_tag, ByteIterator},
	Coordinates0, Coordinates1, Coordinates2, Coordinates3, GeoCollection, GeoFeature,
	GeoProperties, GeoValue, Geometry,
};
use anyhow::{anyhow, bail, Result};
use std::str;
use versatiles_core::utils::{
	parse_array_entries, parse_json_value, parse_number_as, parse_number_as_string,
	parse_object_entries, parse_quoted_json_string,
};

pub fn parse_geojson(json: &str) -> Result<GeoCollection> {
	let mut iter = ByteIterator::from_iterator(json.bytes(), true);
	parse_geojson_collection(&mut iter)
}

pub fn parse_geojson_collection(iter: &mut ByteIterator) -> Result<GeoCollection> {
	let mut features = Vec::new();
	let mut object_type: Option<String> = None;

	parse_object_entries(iter, |key, iter2| {
		match key.as_str() {
			"type" => object_type = Some(parse_quoted_json_string(iter2)?),
			"features" => parse_array_entries(iter2, |iter3| {
				features.push(parse_geojson_feature(iter3)?);
				Ok(())
			})?,
			_ => _ = parse_json_value(iter2)?,
		};
		Ok(())
	})?;

	check_type(object_type, "FeatureCollection")?;

	Ok(GeoCollection { features })
}

fn check_type(object_type: Option<String>, name: &str) -> Result<()> {
	let object_type = object_type.ok_or_else(|| anyhow!("{name} must have a type"))?;

	if object_type.as_str() != name {
		bail!("type must be '{name}'")
	}
	Ok(())
}

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
			_ => _ = parse_json_value(iter2)?,
		};
		Ok(())
	})?;

	check_type(object_type, "Feature")?;

	Ok(GeoFeature {
		id,
		geometry: geometry.ok_or(anyhow!("feature is missing 'geometry'"))?,
		properties: properties.unwrap_or_default(),
	})
}

fn parse_geojson_id(iter: &mut ByteIterator) -> Result<GeoValue> {
	iter.skip_whitespace()?;
	match iter.expect_peeked_byte()? {
		b'"' => parse_quoted_json_string(iter).map(GeoValue::from),
		d if d.is_ascii_digit() => parse_number_as::<u64>(iter).map(GeoValue::UInt),
		c => Err(iter.format_error(&format!(
			"expected a string or integer, but got character '{}'",
			c as char
		))),
	}
}

fn parse_geojson_number(iter: &mut ByteIterator) -> Result<GeoValue> {
	let number = parse_number_as_string(iter)?;

	Ok(if number.contains('.') {
		GeoValue::from(
			number
				.parse::<f64>()
				.map_err(|_| iter.format_error("invalid double"))?,
		)
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

fn parse_geojson_value(iter: &mut ByteIterator) -> Result<GeoValue> {
	iter.skip_whitespace()?;
	match iter.expect_peeked_byte()? {
		b'"' => parse_quoted_json_string(iter).map(GeoValue::from),
		d if d.is_ascii_digit() || d == b'.' || d == b'-' => parse_geojson_number(iter),
		b't' => parse_tag(iter, "true").map(|_| GeoValue::Bool(true)),
		b'f' => parse_tag(iter, "false").map(|_| GeoValue::Bool(false)),
		b'n' => parse_tag(iter, "null").map(|_| GeoValue::Null),
		c => Err(iter.format_error(&format!(
			"expected a string or number, but got character '{}'",
			c as char
		))),
	}
}

fn parse_geojson_properties(iter: &mut ByteIterator) -> Result<GeoProperties> {
	let mut list: Vec<(String, GeoValue)> = Vec::new();
	parse_object_entries(iter, |key, iter2| {
		let value = parse_geojson_value(iter2)?;
		list.push((key, value));
		Ok(())
	})?;

	Ok(GeoProperties::from_iter(list))
}

fn parse_geojson_geometry(iter: &mut ByteIterator) -> Result<Geometry> {
	let mut geometry_type: Option<String> = None;
	let mut coordinates: Option<TemporaryCoordinates> = None;

	parse_object_entries(iter, |key, iter2| {
		match key.as_str() {
			"type" => geometry_type = Some(parse_quoted_json_string(iter2)?),
			"coordinates" => coordinates = Some(parse_geojson_coordinates(iter2)?),
			_ => _ = parse_json_value(iter2)?,
		};
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

enum TemporaryCoordinates {
	V(f64),
	C0(Coordinates0),
	C1(Coordinates1),
	C2(Coordinates2),
	C3(Coordinates3),
}

impl TemporaryCoordinates {
	pub fn unwrap_v(self) -> f64 {
		match self {
			TemporaryCoordinates::V(v) => v,
			_ => panic!("not a value"),
		}
	}
	pub fn unwrap_c0(self) -> Coordinates0 {
		match self {
			TemporaryCoordinates::C0(v) => v,
			_ => panic!("not coordinates0"),
		}
	}
	pub fn unwrap_c1(self) -> Coordinates1 {
		match self {
			TemporaryCoordinates::C1(v) => v,
			_ => panic!("not coordinates1"),
		}
	}
	pub fn unwrap_c2(self) -> Coordinates2 {
		match self {
			TemporaryCoordinates::C2(v) => v,
			_ => panic!("not coordinates2"),
		}
	}
	pub fn unwrap_c3(self) -> Coordinates3 {
		match self {
			TemporaryCoordinates::C3(v) => v,
			_ => panic!("not coordinates3"),
		}
	}
}

fn parse_geojson_coordinates(iter: &mut ByteIterator) -> Result<TemporaryCoordinates> {
	fn recursive(iter: &mut ByteIterator) -> Result<TemporaryCoordinates> {
		use TemporaryCoordinates::*;

		iter.skip_whitespace()?;
		match iter.expect_peeked_byte()? {
			b'[' => {
				let mut list = Vec::new();
				parse_array_entries(iter, |iter2| {
					list.push(recursive(iter2)?);
					Ok(())
				})?;

				if list.is_empty() {
					bail!("empty arrays are not allowed in coordinates")
				};

				let list = match list.first().unwrap() {
					V(_) => {
						if list.len() != 2 {
							bail!("points in coordinates must have exactly two values")
						};
						C0(list
							.into_iter()
							.map(|e| e.unwrap_v())
							.collect::<Vec<f64>>()
							.try_into()
							.unwrap_or_else(|v: Vec<f64>| {
								panic!("Expected a Vec of length {} but it was {}", 2, v.len())
							}))
					}
					C0(_) => C1(list.into_iter().map(|e| e.unwrap_c0()).collect()),
					C1(_) => C2(list.into_iter().map(|e| e.unwrap_c1()).collect()),
					C2(_) => C3(list.into_iter().map(|e| e.unwrap_c2()).collect()),
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
mod tests {
	use super::*;

	#[test]
	fn test_parse_geojson_valid_feature_collection() -> Result<()> {
		let json = r#"
        {
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": {
                        "type": "Point",
                        "coordinates": [102.0, 0.5]
                    },
                    "properties": {
                        "prop0": "value0"
                    }
                }
            ]
        }
        "#;

		let collection = parse_geojson(json)?;
		assert_eq!(collection.features.len(), 1);

		let feature = &collection.features[0];
		assert_eq!(feature.geometry.get_type_name(), "Point");
		if let Geometry::Point(coords) = &feature.geometry {
			assert_eq!(coords.0[0], 102.0);
			assert_eq!(coords.0[1], 0.5);
		}
		assert_eq!(
			feature.properties.get("prop0"),
			Some(&GeoValue::String("value0".to_string()))
		);

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
                    "properties": {
                        "prop0": "value0"
                    }
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
                    "geometry": {
                        "type": "Point",
                        "coordinates": [102.0, 0.5]
                    },
                    "properties": {
                        "prop0": "value0"
                    }
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
                    "geometry": {
                        "type": "Point",
                        "coordinates": [102.0, 0.5]
                    },
                    "properties": {
                        "prop0": "value0"
                    }
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
}
