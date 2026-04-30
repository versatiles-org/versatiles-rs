use crate::geo::{GeoProperties, GeoValue};
use anyhow::{Context, Result, anyhow, ensure};
use std::{collections::HashMap, fmt::Debug, hash::Hash, ops::Div};

#[derive(Clone, PartialEq)]
pub struct VTLPMap<T>
where
	T: Clone + Eq + Hash,
{
	pub list: Vec<T>,
	pub map: HashMap<T, u32>,
}

impl<T> VTLPMap<T>
where
	T: Clone + Debug + Eq + Hash,
{
	pub fn new(list: Vec<T>) -> VTLPMap<T> {
		#[allow(clippy::cast_possible_truncation)]
		let map = list.iter().enumerate().map(|(i, e)| (e.clone(), i as u32)).collect();
		VTLPMap { list, map }
	}

	pub fn add(&mut self, entry: T) -> u32 {
		if let Some(index) = self.map.get(&entry) {
			return *index;
		}
		#[allow(clippy::cast_possible_truncation)]
		let index = self.list.len() as u32;
		self.map.insert(entry.clone(), index);
		self.list.push(entry);
		index
	}

	pub fn iter(&self) -> impl Iterator<Item = &T> + '_ {
		self.list.iter()
	}

	pub fn find(&self, entry: &T) -> Result<u32> {
		self
			.map
			.get(entry)
			.ok_or_else(|| anyhow!("entry '{entry:?}' not found"))
			.copied()
	}

	pub fn get(&self, id: u32) -> Result<&T> {
		self
			.list
			.get(id as usize)
			.ok_or_else(|| anyhow!("id '{id:?}' not found"))
	}
}

impl<T: Clone + Debug + Eq + Hash> Default for VTLPMap<T> {
	fn default() -> VTLPMap<T> {
		VTLPMap::new(vec![])
	}
}

impl<T> From<&[&str]> for VTLPMap<T>
where
	T: Clone + Debug + Eq + Hash + From<String>,
{
	fn from(value: &[&str]) -> Self {
		VTLPMap::new(value.iter().map(|v| T::from((*v).to_string())).collect())
	}
}

impl<T> Debug for VTLPMap<T>
where
	T: Clone + Debug + Eq + Hash,
{
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(&self.list).finish()
	}
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyManager {
	pub key: VTLPMap<String>,
	pub val: VTLPMap<GeoValue>,
}

impl PropertyManager {
	pub fn new() -> Self {
		Self {
			key: VTLPMap::default(),
			val: VTLPMap::default(),
		}
	}

	#[cfg(test)]
	pub fn from_slices(keys: &[&str], values: &[&str]) -> Self {
		Self {
			key: VTLPMap::from(keys),
			val: VTLPMap::from(values),
		}
	}

	pub fn add_key(&mut self, key: String) -> u32 {
		self.key.add(key)
	}

	pub fn add_val(&mut self, value: GeoValue) -> u32 {
		self.val.add(value)
	}

	pub fn iter_key(&self) -> impl Iterator<Item = &String> + '_ {
		self.key.iter()
	}

	pub fn iter_val(&self) -> impl Iterator<Item = &GeoValue> + '_ {
		self.val.iter()
	}

	pub fn from_iter<'a, I>(geo_property_iter: I) -> Self
	where
		I: IntoIterator<Item = &'a GeoProperties>,
	{
		let mut key_map: HashMap<String, u32> = HashMap::new();
		let mut val_map: HashMap<GeoValue, u32> = HashMap::new();

		for properties in geo_property_iter {
			for (k, v) in properties.iter() {
				key_map.entry(k.clone()).and_modify(|n| *n += 1).or_insert(0);
				val_map.entry(v.clone()).and_modify(|n| *n += 1).or_insert(0);
			}
		}

		fn make_lookup<T>(map: HashMap<T, u32>) -> VTLPMap<T>
		where
			T: Clone + Debug + Eq + Hash + Ord,
		{
			let mut vec: Vec<(T, u32)> = map.into_iter().collect();
			vec.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
			let list: Vec<T> = vec.into_iter().map(|(v, _)| v).collect();
			VTLPMap::new(list)
		}

		Self {
			key: make_lookup(key_map),
			val: make_lookup(val_map),
		}
	}

	pub fn encode_tag_ids(&mut self, properties: GeoProperties) -> Vec<u32> {
		let mut tag_ids: Vec<u32> = Vec::new();

		for (key, val) in properties {
			tag_ids.push(self.key.add(key));
			tag_ids.push(self.val.add(val));
		}

		tag_ids
	}

	pub fn decode_tag_ids(&self, tag_ids: &[u32]) -> Result<GeoProperties> {
		ensure!(tag_ids.len().is_multiple_of(2), "Tag IDs must be even");
		let mut properties = GeoProperties::new();

		for i in 0..tag_ids.len().div(2) {
			let tag_key = tag_ids[i * 2];
			let tag_val = tag_ids[i * 2 + 1];
			properties.insert(
				self.key.get(tag_key).context("Failed to get property key")?.to_owned(),
				self.val.get(tag_val).context("Failed to get property value")?.clone(),
			);
		}
		Ok(properties)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn vtlp_add_dedupes_and_assigns_stable_indices() {
		let mut m: VTLPMap<String> = VTLPMap::default();
		assert_eq!(m.add("a".to_string()), 0);
		assert_eq!(m.add("b".to_string()), 1);
		// Same entry returns the existing id, no new slot.
		assert_eq!(m.add("a".to_string()), 0);
		assert_eq!(m.list, vec!["a", "b"]);
	}

	#[test]
	fn vtlp_get_out_of_bounds_errors() {
		let m: VTLPMap<String> = VTLPMap::from(["a", "b"].as_slice());
		assert!(m.get(2).is_err());
		assert!(m.get(u32::MAX).is_err());
	}

	#[test]
	fn vtlp_find_missing_errors() {
		let m: VTLPMap<String> = VTLPMap::from(["a"].as_slice());
		assert!(m.find(&"a".to_string()).is_ok());
		assert!(m.find(&"z".to_string()).is_err());
	}

	#[test]
	fn encode_decode_round_trip() {
		let mut pm = PropertyManager::new();
		let props: GeoProperties = vec![
			("name", GeoValue::from("Berlin")),
			("pop", GeoValue::from(3_645_000_u64)),
		]
		.into();
		let tag_ids = pm.encode_tag_ids(props.clone());
		assert_eq!(tag_ids.len(), 4, "two key-value pairs → four ids");
		let decoded = pm.decode_tag_ids(&tag_ids).unwrap();
		assert_eq!(decoded.get("name"), props.get("name"));
		assert_eq!(decoded.get("pop"), props.get("pop"));
	}

	#[test]
	fn decode_rejects_odd_length_tag_ids() {
		let pm = PropertyManager::from_slices(&["k"], &["v"]);
		// 3 ids = 1.5 pairs — must error rather than silently dropping or
		// reading off the end of the value table.
		let err = pm.decode_tag_ids(&[0, 0, 0]).unwrap_err();
		assert!(format!("{err:#}").contains("even"), "{err:#}");
	}

	#[test]
	fn decode_rejects_unknown_tag_index() {
		let pm = PropertyManager::from_slices(&["k"], &["v"]);
		// Key id 5 doesn't exist.
		let err = pm.decode_tag_ids(&[5, 0]).unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("property key"), "{msg}");
	}

	#[test]
	fn encode_dedupes_repeated_keys_across_features() {
		// Two features, same key. The shared key should land at index 0 once,
		// not twice — that's the whole point of the property manager.
		let mut pm = PropertyManager::new();
		let p1: GeoProperties = vec![("name", GeoValue::from("A"))].into();
		let p2: GeoProperties = vec![("name", GeoValue::from("B"))].into();
		let ids1 = pm.encode_tag_ids(p1);
		let ids2 = pm.encode_tag_ids(p2);
		assert_eq!(ids1[0], ids2[0], "key index should be shared");
		assert_ne!(ids1[1], ids2[1], "value index should differ");
		assert_eq!(pm.key.list, vec!["name"]);
		assert_eq!(pm.val.list.len(), 2);
	}
}
