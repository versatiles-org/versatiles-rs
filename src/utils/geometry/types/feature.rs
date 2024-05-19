#![allow(dead_code)]

use std::fmt::Debug;

use super::*;

#[derive(Debug, PartialEq)]
pub struct Feature {
	pub id: Option<u64>,
	pub geometry: Geometry,
	pub properties: Option<GeoProperties>,
}

impl Feature {
	pub fn new(geometry: Geometry) -> Self {
		Feature {
			id: None,
			geometry,
			properties: None,
		}
	}
	pub fn set_id(&mut self, id: u64) {
		self.id = Some(id);
	}
	pub fn set_properties(&mut self, properties: GeoProperties) {
		self.properties = Some(properties);
	}
}
