#![allow(dead_code)]

use std::fmt::Debug;

use super::*;

#[derive(Clone, Debug)]
pub struct Feature {
	pub id: Option<u64>,
	pub geometry: Geometry,
	pub properties: Option<GeoProperties>,
}

impl Feature {
	pub fn new(geometry: Geometry) -> Self {
		Self {
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

	#[cfg(test)]
	pub fn new_example() -> Self {
		use std::collections::HashMap;

		Self {
			id: Some(13),
			geometry: Geometry::new_example(),
			properties: Some(HashMap::from([
				(String::from("name"), GeoValue::from("Berlin")),
				(String::from("population"), GeoValue::from(3755251)),
				(
					String::from("it_would_actually_be_quite_a_nice_place_if_so_many_hipsters_hadn_t_moved_there"),
					GeoValue::from(true),
				),
			])),
		}
	}
}
