#![allow(dead_code)]

use super::GeoValue;
use std::{collections::HashMap, fmt::Debug};

#[derive(Clone, PartialEq)]
pub struct GeoProperties {
	properties: HashMap<String, GeoValue>,
}

impl GeoProperties {
	pub fn new() -> GeoProperties {
		GeoProperties {
			properties: HashMap::new(),
		}
	}
	pub fn insert(&mut self, key: String, value: GeoValue) {
		self.properties.insert(key, value);
	}
	pub fn update(&mut self, new_properties: GeoProperties) {
		for (k, v) in new_properties.into_iter() {
			self.properties.insert(k, v);
		}
	}
	pub fn remove(&mut self, key: &str) {
		self.properties.remove(key);
	}
	pub fn iter(&self) -> std::collections::hash_map::Iter<String, GeoValue> {
		self.properties.iter()
	}
	pub fn into_iter(self) -> std::collections::hash_map::IntoIter<String, GeoValue> {
		self.properties.into_iter()
	}
	pub fn get(&self, key: &str) -> Option<&GeoValue> {
		self.properties.get(key)
	}
}

impl From<Vec<(&str, GeoValue)>> for GeoProperties {
	fn from(value: Vec<(&str, GeoValue)>) -> Self {
		GeoProperties {
			properties: value.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
		}
	}
}

impl Debug for GeoProperties {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut fields = self.clone().into_iter().collect::<Vec<(String, GeoValue)>>();
		fields.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

		f.debug_map().entries(fields).finish()
	}
}
