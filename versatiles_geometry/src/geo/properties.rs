#![allow(dead_code)]

use super::GeoValue;
use std::{
	collections::{btree_map, BTreeMap},
	fmt::Debug,
};

#[derive(Clone, PartialEq)]
pub struct GeoProperties {
	properties: BTreeMap<String, GeoValue>,
}

impl Default for GeoProperties {
	fn default() -> Self {
		Self::new()
	}
}

impl GeoProperties {
	pub fn new() -> GeoProperties {
		GeoProperties {
			properties: BTreeMap::new(),
		}
	}
	pub fn insert(&mut self, key: String, value: GeoValue) {
		self.properties.insert(key, value);
	}
	pub fn update(&mut self, new_properties: &GeoProperties) {
		for (k, v) in new_properties.iter() {
			self.properties.insert(k.to_string(), v.clone());
		}
	}
	pub fn remove(&mut self, key: &str) {
		self.properties.remove(key);
	}
	pub fn get(&self, key: &str) -> Option<&GeoValue> {
		self.properties.get(key)
	}
	pub fn iter(&self) -> btree_map::Iter<String, GeoValue> {
		self.properties.iter()
	}
}

impl IntoIterator for GeoProperties {
	type Item = (String, GeoValue);
	type IntoIter = btree_map::IntoIter<String, GeoValue>;
	fn into_iter(self) -> Self::IntoIter {
		self.properties.into_iter()
	}
}

impl From<Vec<(&str, GeoValue)>> for GeoProperties {
	fn from(value: Vec<(&str, GeoValue)>) -> Self {
		GeoProperties {
			properties: value.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
		}
	}
}

impl From<Vec<(&str, &str)>> for GeoProperties {
	fn from(value: Vec<(&str, &str)>) -> Self {
		GeoProperties {
			properties: value
				.into_iter()
				.map(|(k, v)| (k.to_string(), GeoValue::from(v)))
				.collect(),
		}
	}
}

impl FromIterator<(String, GeoValue)> for GeoProperties {
	fn from_iter<T: IntoIterator<Item = (String, GeoValue)>>(iter: T) -> Self {
		GeoProperties {
			properties: BTreeMap::from_iter(iter),
		}
	}
}

impl Debug for GeoProperties {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let fields = self
			.properties
			.clone()
			.into_iter()
			.collect::<Vec<(String, GeoValue)>>();

		f.debug_map().entries(fields).finish()
	}
}
