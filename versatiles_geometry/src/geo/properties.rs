#![allow(dead_code)]

use versatiles_core::json::JsonObject;

use super::GeoValue;
use std::{
	collections::{BTreeMap, btree_map},
	fmt::Debug,
};

#[derive(Clone, PartialEq)]
pub struct GeoProperties(pub BTreeMap<String, GeoValue>);

impl Default for GeoProperties {
	fn default() -> Self {
		Self::new()
	}
}

impl GeoProperties {
	pub fn new() -> GeoProperties {
		GeoProperties(BTreeMap::new())
	}
	pub fn insert(&mut self, key: String, value: GeoValue) {
		self.0.insert(key, value);
	}
	pub fn update(&mut self, new_properties: &GeoProperties) {
		for (k, v) in new_properties.iter() {
			self.0.insert(k.to_string(), v.clone());
		}
	}
	pub fn remove(&mut self, key: &str) {
		self.0.remove(key);
	}
	pub fn get(&self, key: &str) -> Option<&GeoValue> {
		self.0.get(key)
	}
	pub fn iter(&self) -> btree_map::Iter<'_, String, GeoValue> {
		self.0.iter()
	}
	pub fn retain<F>(&mut self, f: F)
	where
		F: Fn(&String, &GeoValue) -> bool,
	{
		self.0.retain(|k, v| f(k, v));
	}
	pub fn to_json(&self) -> JsonObject {
		let mut obj = JsonObject::new();
		for (k, v) in &self.0 {
			obj.set(k, v.to_json());
		}
		obj
	}
}

impl IntoIterator for GeoProperties {
	type Item = (String, GeoValue);
	type IntoIter = btree_map::IntoIter<String, GeoValue>;
	fn into_iter(self) -> Self::IntoIter {
		self.0.into_iter()
	}
}

impl From<Vec<(&str, GeoValue)>> for GeoProperties {
	fn from(value: Vec<(&str, GeoValue)>) -> Self {
		GeoProperties(value.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
	}
}

impl From<Vec<(&str, &str)>> for GeoProperties {
	fn from(value: Vec<(&str, &str)>) -> Self {
		GeoProperties(
			value
				.into_iter()
				.map(|(k, v)| (k.to_string(), GeoValue::from(v)))
				.collect(),
		)
	}
}

impl FromIterator<(String, GeoValue)> for GeoProperties {
	fn from_iter<T: IntoIterator<Item = (String, GeoValue)>>(iter: T) -> Self {
		GeoProperties(BTreeMap::from_iter(iter))
	}
}

impl Debug for GeoProperties {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let fields = self.0.clone().into_iter().collect::<Vec<(String, GeoValue)>>();

		f.debug_map().entries(fields).finish()
	}
}
