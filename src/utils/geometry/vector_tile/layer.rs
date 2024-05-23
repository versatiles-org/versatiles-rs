#![allow(dead_code)]

use super::{feature::VectorTileFeature, value::GeoValuePBF};
use crate::{
	types::{Blob, ValueReader, ValueWriter, ValueWriterBlob},
	utils::geometry::basic::{Feature, GeoProperties, GeoValue},
};
use anyhow::{anyhow, bail, ensure, Context, Result};
use byteorder::LE;
use itertools::Itertools;
use std::{collections::HashMap, ops::Div};

#[derive(Debug, Default, PartialEq)]
pub struct VectorTileLayer {
	pub extent: u32,
	pub features: Vec<VectorTileFeature>,
	pub name: String,
	pub property_keys: Vec<String>,
	pub property_values: Vec<GeoValue>,
	pub version: u32,
}

impl VectorTileLayer {
	pub fn read(reader: &mut dyn ValueReader<'_, LE>) -> Result<VectorTileLayer> {
		let mut extent = 4096;
		let mut features: Vec<VectorTileFeature> = Vec::new();
		let mut name = None;
		let mut property_keys = Vec::new();
		let mut property_values = Vec::new();
		let mut version = 1;

		while reader.has_remaining() {
			match reader.read_pbf_key().context("Failed to read PBF key")? {
				(1, 2) => name = Some(reader.read_pbf_string().context("Failed to read layer name")?),
				(2, 2) => features.push(
					VectorTileFeature::read(
						reader
							.get_pbf_sub_reader()
							.context("Failed to get PBF sub-reader for feature")?
							.as_mut(),
					)
					.context("Failed to read VectorTileFeature")?,
				),
				(3, 2) => property_keys.push(reader.read_pbf_string().context("Failed to read property key")?),
				(4, 2) => property_values.push(
					GeoValue::read(
						reader
							.get_pbf_sub_reader()
							.context("Failed to get PBF sub-reader for property value")?
							.as_mut(),
					)
					.context("Failed to read GeoValue")?,
				),
				(5, 0) => extent = reader.read_varint().context("Failed to read extent")? as u32,
				(15, 0) => version = reader.read_varint().context("Failed to read version")? as u32,
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		Ok(VectorTileLayer {
			extent,
			features,
			name: name
				.ok_or(anyhow!("Layer name is required"))
				.context("Failed to get layer name")?,
			property_keys,
			property_values,
			version,
		})
	}

	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_le();

		writer
			.write_pbf_key(1, 2)
			.context("Failed to write PBF key for layer name")?;
		writer
			.write_pbf_string(&self.name)
			.context("Failed to write layer name")?;

		for feature in self.features.iter() {
			writer
				.write_pbf_key(2, 2)
				.context("Failed to write PBF key for feature")?;
			writer
				.write_pbf_blob(&feature.to_blob().context("Failed to convert feature to blob")?)
				.context("Failed to write feature blob")?;
		}

		for key in self.property_keys.iter() {
			writer
				.write_pbf_key(3, 2)
				.context("Failed to write PBF key for property key")?;
			writer.write_pbf_string(key).context("Failed to write property key")?;
		}

		for value in self.property_values.iter() {
			writer
				.write_pbf_key(4, 2)
				.context("Failed to write PBF key for property value")?;
			writer
				.write_pbf_blob(&value.to_blob().context("Failed to convert property value to blob")?)
				.context("Failed to write property value blob")?;
		}

		if self.extent != 4096 {
			writer
				.write_pbf_key(5, 0)
				.context("Failed to write PBF key for extent")?;
			writer
				.write_varint(self.extent as u64)
				.context("Failed to write extent")?;
		}

		if self.version != 1 {
			writer
				.write_pbf_key(15, 0)
				.context("Failed to write PBF key for version")?;
			writer
				.write_varint(self.version as u64)
				.context("Failed to write version")?;
		}

		Ok(writer.into_blob())
	}

	pub fn translate_tag_ids(&self, tag_ids: &[u32]) -> Result<GeoProperties> {
		ensure!(tag_ids.len() % 2 == 0, "Tag IDs must be even");
		let mut properties = GeoProperties::new();

		for i in 0..tag_ids.len().div(2) {
			let tag_key = tag_ids[i * 2] as usize;
			let tag_val = tag_ids[i * 2 + 1] as usize;
			properties.insert(
				self
					.property_keys
					.get(tag_key)
					.ok_or(anyhow!("Property key '{tag_key}' not found"))
					.context("Failed to get property key")?
					.to_owned(),
				self
					.property_values
					.get(tag_val)
					.ok_or(anyhow!("Property value '{tag_val}' not found"))
					.context("Failed to get property value")?
					.clone(),
			);
		}
		Ok(properties)
	}

	pub fn to_features(&self) -> Result<Vec<Feature>> {
		let mut features = Vec::new();
		for feature in &self.features {
			features.push(
				feature
					.to_feature(self)
					.context("Failed to convert VectorTileFeature to MultiFeature")?,
			);
		}
		Ok(features)
	}

	pub fn from_features(name: String, features: Vec<Feature>, extent: u32, version: u32) -> Result<VectorTileLayer> {
		let mut prop_key_map: HashMap<String, u32> = HashMap::new();
		let mut prop_val_map: HashMap<GeoValue, u32> = HashMap::new();
		let mut features_vec: Vec<VectorTileFeature> = Vec::new();

		for feature in features.iter() {
			if let Some(properties) = &feature.properties {
				for (k, v) in properties.iter() {
					prop_key_map.entry(k.clone()).and_modify(|n| *n += 1).or_insert(0);
					prop_val_map.entry(v.clone()).and_modify(|n| *n += 1).or_insert(0);
				}
			}
		}

		fn make_lookup<T>(map: HashMap<T, u32>) -> (Vec<T>, HashMap<T, u32>)
		where
			T: Clone + Eq + std::hash::Hash + Ord,
		{
			let mut vec: Vec<(T, u32)> = map.into_iter().collect();
			vec.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
			let vec = vec.into_iter().map(|(v, _)| v).collect_vec();
			let map: HashMap<T, u32> = HashMap::from_iter(vec.iter().enumerate().map(|(i, v)| (v.clone(), i as u32)));
			(vec, map)
		}

		let (prop_key_vec, prop_key_map) = make_lookup(prop_key_map);
		let (prop_val_vec, prop_val_map) = make_lookup(prop_val_map);

		for feature in features {
			let mut tag_ids: Vec<u32> = Vec::new();

			if let Some(properties) = feature.properties {
				for (key, val) in properties.iter() {
					tag_ids.push(*prop_key_map.get(key).unwrap());
					tag_ids.push(*prop_val_map.get(val).unwrap());
				}
			}

			features_vec.push(VectorTileFeature::from_geometry(
				feature.id,
				tag_ids,
				&feature.geometry,
			)?);
		}

		Ok(VectorTileLayer {
			extent,
			features: features_vec,
			name,
			property_keys: prop_key_vec,
			property_values: prop_val_vec,
			version,
		})
	}

	#[cfg(test)]
	pub fn new_example() -> Self {
		VectorTileLayer::from_features(String::from("layer1"), vec![Feature::new_example()], 4096, 1).unwrap()
	}
}

#[cfg(test)]
mod tests {
	use crate::types::ValueReaderSlice;

	use super::*;

	#[test]
	fn test_read_vector_tile_layer() -> Result<()> {
		// Example data for a vector tile layer
		let data = vec![
			0x0A, 0x05, b'h', b'e', b'l', b'l', b'o', // name: "hello"
			18, 50, 8, 3, 18, 2, 1, 2, 24, 3, 34, 40, 9, 0, 0, 18, 10, 0, 3, 8, 7, 9, 1, 5, 18, 2, 2, 0, 1, 7, 9, 6, 1,
			26, 6, 0, 0, 8, 5, 0, 7, 9, 2, 5, 26, 0, 4, 2, 0, 0, 3, 7, // feature
			0x1A, 0x03, b'k', b'e', b'y', // property key: "key"
			0x22, 0x04, 0x0A, 0x02, b'v', b'l', // property value: "vl"
		];
		let mut reader = ValueReaderSlice::new_le(&data);
		let layer = VectorTileLayer::read(&mut reader)?;

		assert_eq!(layer.name, "hello");
		assert_eq!(layer.features.len(), 1);
		assert_eq!(layer.property_keys, vec!["key"]);
		assert_eq!(layer.property_values, vec![GeoValue::from("vl")]);
		assert_eq!(layer.extent, 4096);
		assert_eq!(layer.version, 1);
		Ok(())
	}

	#[test]
	fn test_to_blob() -> Result<()> {
		let layer = VectorTileLayer {
			name: "hello".to_string(),
			features: vec![VectorTileFeature::new_example()],
			property_keys: vec!["key".to_string()],
			property_values: vec![GeoValue::from("vl")],
			extent: 4096,
			version: 1,
		};
		let blob = layer.to_blob()?;
		let expected_data = vec![
			0x0A, 0x05, b'h', b'e', b'l', b'l', b'o', // name: "hello"
			18, 50, 8, 3, 18, 2, 1, 2, 24, 3, 34, 40, 9, 0, 0, 18, 10, 0, 3, 8, 7, 9, 1, 5, 18, 2, 2, 0, 1, 7, 9, 6, 1,
			26, 6, 0, 0, 8, 5, 0, 7, 9, 2, 5, 26, 0, 4, 2, 0, 0, 3, 7, // feature
			0x1A, 0x03, b'k', b'e', b'y', // property key: "key"
			0x22, 0x04, 0x0A, 0x02, b'v', b'l', // property value: "vl"
		];
		assert_eq!(blob.into_vec(), expected_data);
		Ok(())
	}

	#[test]
	fn test_translate_tag_ids() -> Result<()> {
		let layer = VectorTileLayer {
			name: "hello".to_string(),
			features: vec![],
			property_keys: vec!["key".to_string()],
			property_values: vec![GeoValue::from("vl")],
			extent: 4096,
			version: 1,
		};
		let tag_ids = vec![0, 0]; // (key, value)
		let properties = layer.translate_tag_ids(&tag_ids)?;
		let expected_properties = GeoProperties::from(vec![("key", GeoValue::from("vl"))]);
		assert_eq!(properties, expected_properties);
		Ok(())
	}

	#[test]
	fn test_to_features() -> Result<()> {
		let feature = Feature::new_example();
		let layer = VectorTileLayer::from_features("hello".to_string(), vec![feature.clone()], 2048, 3)?;
		let features = layer.to_features()?;
		println!("{:?}", features[0].properties);
		assert_eq!(features.len(), 1);
		assert_eq!(
			features[0].properties.as_ref().unwrap().get("name").unwrap(),
			&GeoValue::from("Berlin")
		);
		Ok(())
	}

	#[test]
	fn test_from_features() -> Result<()> {
		let features = vec![Feature::new_example()];
		let layer = VectorTileLayer::from_features("hello".to_string(), features, 4096, 1)?;
		assert_eq!(layer.name, "hello");
		assert_eq!(layer.features.len(), 1);
		assert_eq!(
			layer.property_keys,
			vec![
				"it_would_actually_be_quite_a_nice_place_if_so_many_hipsters_hadn_t_moved_there",
				"name",
				"population"
			]
		);
		assert_eq!(
			layer.property_values,
			vec![GeoValue::from("Berlin"), GeoValue::from(3755251), GeoValue::from(true)]
		);
		assert_eq!(layer.extent, 4096);
		assert_eq!(layer.version, 1);
		Ok(())
	}
}
