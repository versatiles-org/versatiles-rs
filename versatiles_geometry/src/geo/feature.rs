#![allow(dead_code)]

use super::{GeoProperties, GeoValue, Geometry};
use lazy_static::lazy_static;
use std::{fmt::Debug, mem::swap};
use versatiles_core::json::{JsonObject, JsonValue};

lazy_static! {
	static ref DUMMY_GEOMETRY: Geometry = Geometry::new_multi_point::<Vec<(f64, f64)>>(vec![]);
}

#[derive(Clone, Debug)]
pub struct GeoFeature {
	pub id: Option<GeoValue>,
	pub geometry: Geometry,
	pub properties: GeoProperties,
}

impl GeoFeature {
	#[must_use]
	pub fn new(geometry: Geometry) -> Self {
		Self {
			id: None,
			geometry,
			properties: GeoProperties::new(),
		}
	}

	pub fn set_id(&mut self, id: GeoValue) {
		self.id = Some(id);
	}

	pub fn set_properties(&mut self, properties: GeoProperties) {
		self.properties = properties;
	}

	pub fn set_property<T>(&mut self, key: String, value: T)
	where
		GeoValue: From<T>,
	{
		self.properties.insert(key, GeoValue::from(value));
	}

	pub fn to_single_geometry(&mut self) {
		if self.geometry.is_single_geometry() {
			return;
		}
		let mut geometry = DUMMY_GEOMETRY.clone();
		swap(&mut geometry, &mut self.geometry);
		self.geometry = geometry.into_single_geometry();
	}

	pub fn to_multi_geometry(&mut self) {
		if self.geometry.is_multi_geometry() {
			return;
		}
		let mut geometry = DUMMY_GEOMETRY.clone();
		swap(&mut geometry, &mut self.geometry);
		self.geometry = geometry.into_multi_geometry();
	}

	pub fn to_json(&self, precision: Option<u8>) -> JsonObject {
		let mut json = JsonObject::new();
		json.set("type", JsonValue::from("Feature"));
		if let Some(id) = &self.id {
			json.set("id", id.to_json());
		}
		json.set("geometry", self.geometry.to_json(precision));
		json.set("properties", self.properties.to_json());
		json
	}

	#[cfg(test)]
	pub fn new_example() -> Self {
		Self {
			id: Some(GeoValue::from(13)),
			geometry: Geometry::new_example(),
			properties: GeoProperties::from(vec![
				("name", GeoValue::from("Nice")),
				("population", GeoValue::from(348085)),
				("is_nice", GeoValue::from(true)),
			]),
		}
	}
}

impl From<geo::MultiPolygon<f64>> for GeoFeature {
	fn from(geometry: geo::MultiPolygon<f64>) -> Self {
		Self::new(Geometry::from(geometry))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo::{MultiPolygon, polygon};

	#[test]
	fn new_sets_defaults() {
		let geom = Geometry::new_example();
		let f = GeoFeature::new(geom.clone());
		assert!(f.id.is_none());
		assert_eq!(f.geometry, geom);
		assert!(f.properties.is_empty());
	}

	#[test]
	fn set_id_and_properties_work() {
		let geom = Geometry::new_example();
		let mut f = GeoFeature::new(geom);
		f.set_id(GeoValue::from(42));
		assert_eq!(f.id, Some(GeoValue::from(42)));

		let mut props = GeoProperties::new();
		props.insert("name".into(), GeoValue::from("Nice"));
		props.insert("population".into(), GeoValue::from(348_085));
		f.set_properties(props.clone());
		assert_eq!(f.properties, props);
	}

	#[test]
	fn set_property_inserts_values_of_various_types() {
		let geom = Geometry::new_example();
		let mut f = GeoFeature::new(geom);
		f.set_property("a".into(), 1u32);
		f.set_property("b".into(), "text");
		f.set_property("c".into(), true);
		assert_eq!(f.properties.get("a"), Some(&GeoValue::from(1u32)));
		assert_eq!(f.properties.get("b"), Some(&GeoValue::from("text")));
		assert_eq!(f.properties.get("c"), Some(&GeoValue::from(true)));
	}

	#[test]
	fn to_json_contains_type_geometry_properties_and_optional_id() {
		// with id
		let mut f = GeoFeature::new_example();
		let j = f.to_json(None);
		assert_eq!(j.get_string("type").unwrap(), Some("Feature".into()));
		assert!(j.get_number("id").unwrap().is_some());
		assert!(j.get_object("geometry").unwrap().is_some());
		assert!(j.get_object("properties").unwrap().is_some());

		// without id
		f.id = None;
		let j2 = f.to_json(None);
		assert_eq!(j2.get_string("type").unwrap(), Some("Feature".into()));
		assert!(j2.get_number("id").unwrap().is_none());
	}

	#[test]
	fn from_multipolygon_builds_feature() {
		// a simple square polygon
		let poly = polygon![(x: 0.0, y: 0.0), (x: 1.0, y: 0.0), (x: 1.0, y: 1.0), (x: 0.0, y: 1.0), (x: 0.0, y: 0.0)];
		let mp = MultiPolygon(vec![poly]);
		let f: GeoFeature = mp.into();
		// id defaults to None
		assert!(f.id.is_none());
		// properties start empty
		assert!(f.properties.is_empty());
		// geometry is present and serializable
		let j = f.to_json(None);
		assert_eq!(j.get_string("type").unwrap(), Some("Feature".into()));
		assert!(j.get_object("geometry").unwrap().is_some());
	}

	#[test]
	fn feature_example_contains_expected_values() {
		let f = GeoFeature::new_example();
		assert_eq!(
			f.to_json(None)
				.stringify_pretty_multi_line(100, 0)
				.split('\n')
				.collect::<Vec<_>>(),
			[
				"{",
				"  \"geometry\": {",
				"    \"coordinates\": [",
				"      [[[0, 0], [5, 0], [2.5, 4], [0, 0]], [[2, 1], [2.5, 2], [3, 1], [2, 1]]],",
				"      [[[6, 0], [9, 0], [9, 4], [6, 4], [6, 0]], [[7, 1], [7, 3], [8, 3], [8, 1], [7, 1]]]",
				"    ],",
				"    \"type\": \"MultiPolygon\"",
				"  },",
				"  \"id\": 13,",
				"  \"properties\": { \"is_nice\": true, \"name\": \"Nice\", \"population\": 348085 },",
				"  \"type\": \"Feature\"",
				"}"
			]
		);
	}
}
