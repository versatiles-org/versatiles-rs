#![allow(dead_code)]

use std::fmt::Debug;

use super::*;

#[derive(Clone, Debug)]
pub struct GeoFeature {
	pub id: Option<u64>,
	pub geometry: Geometry,
	pub properties: GeoProperties,
}

impl GeoFeature {
	pub fn new(geometry: Geometry) -> Self {
		Self {
			id: None,
			geometry,
			properties: GeoProperties::new(),
		}
	}

	pub fn set_id(&mut self, id: u64) {
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

	#[cfg(test)]
	pub fn new_example() -> Self {
		Self {
			id: Some(13),
			geometry: Geometry::new_example(),
			properties: GeoProperties::from(vec![
				("name", GeoValue::from("Nice")),
				("population", GeoValue::from(348085)),
				("is_nice", GeoValue::from(true)),
			]),
		}
	}
}
