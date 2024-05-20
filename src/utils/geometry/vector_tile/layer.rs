#![allow(dead_code)]

use super::{feature::VectorTileFeature, utils::BlobWriterPBF, value::GeoValuePBF};
use crate::{
	types::{Blob, ValueReader},
	utils::{
		geometry::basic::{Feature, GeoProperties, GeoValue},
		BlobWriter,
	},
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
		let mut writer = BlobWriter::new_le();

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
		let mut attributes = GeoProperties::new();
		for i in 0..tag_ids.len().div(2) {
			let tag_key = tag_ids[i * 2] as usize;
			let tag_val = tag_ids[i * 2 + 1] as usize;
			attributes.insert(
				self
					.property_keys
					.get(tag_key)
					.ok_or(anyhow!("Property key not found"))
					.context("Failed to get property key")?
					.to_owned(),
				self
					.property_values
					.get(tag_val)
					.ok_or(anyhow!("Property value not found"))
					.context("Failed to get property value")?
					.clone(),
			);
		}
		Ok(attributes)
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
				for (k, v) in properties {
					prop_key_map.entry(k.clone()).and_modify(|n| *n += 1).or_insert(0);
					prop_val_map.entry(v.clone()).and_modify(|n| *n += 1).or_insert(0);
				}
			}
		}

		fn make_lookup<T>(map: HashMap<T, u32>) -> (Vec<T>, HashMap<T, u32>)
		where
			T: Clone + Eq + std::hash::Hash,
		{
			let mut vec: Vec<(T, u32)> = map.into_iter().collect();
			vec.sort_unstable_by_key(|(_, n)| *n);
			let vec = vec.into_iter().map(|(v, _)| v).collect_vec();
			let map: HashMap<T, u32> = HashMap::from_iter(vec.iter().enumerate().map(|(i, v)| (v.clone(), i as u32)));
			(vec, map)
		}

		let (prop_key_vec, prop_key_map) = make_lookup(prop_key_map);
		let (prop_val_vec, prop_val_map) = make_lookup(prop_val_map);

		for feature in features {
			let mut tag_ids: Vec<u32> = Vec::new();

			if let Some(properties) = feature.properties {
				for (key, val) in properties {
					tag_ids.push(*prop_key_map.get(&key).unwrap());
					tag_ids.push(*prop_val_map.get(&val).unwrap());
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
}
