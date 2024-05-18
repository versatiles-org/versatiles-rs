use crate::utils::geometry::types::{GeoProperties, GeoValue};
use anyhow::{anyhow, ensure, Result};
use std::ops::Div;

pub struct AttributeLookup {
	pub keys: Vec<String>,
	pub values: Vec<GeoValue>,
}

impl AttributeLookup {
	pub fn new() -> AttributeLookup {
		AttributeLookup {
			keys: Vec::new(),
			values: Vec::new(),
		}
	}
	pub fn add_key(&mut self, key: String) {
		self.keys.push(key);
	}
	pub fn add_value(&mut self, value: GeoValue) {
		self.values.push(value);
	}
	pub fn translate_tag_ids(&self, tag_ids: &[u32]) -> Result<GeoProperties> {
		ensure!(tag_ids.len() % 2 == 0, "must be even");
		let mut attributes = GeoProperties::new();
		for i in 0..tag_ids.len().div(2) {
			let tag_key = tag_ids[i * 2] as usize;
			let tag_val = tag_ids[i * 2 + 1] as usize;
			attributes.insert(
				self.keys.get(tag_key).ok_or(anyhow!("key not found"))?.to_owned(),
				self.values.get(tag_val).ok_or(anyhow!("value not found"))?.clone(),
			);
		}
		Ok(attributes)
	}
}
