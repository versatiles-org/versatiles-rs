#![allow(dead_code)]

use super::*;
use std::fmt::Debug;

#[derive(Clone, Debug)]
pub struct GeoFeature {
	pub id: Option<GeoValue>,
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
