#![allow(dead_code)]

use super::{feature::VectorTileFeature, utils::BlobReaderPBF, utils::BlobWriterPBF, value::GeoValuePBF};
use crate::{
	types::Blob,
	utils::{
		geometry::types::{GeoProperties, GeoValue},
		BlobReader, BlobWriter,
	},
};
use anyhow::{anyhow, bail, ensure, Result};
use byteorder::LE;
use std::ops::Div;

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
	pub fn read(reader: &mut BlobReader<LE>) -> Result<VectorTileLayer> {
		let mut extent = 4096;
		let mut features: Vec<VectorTileFeature> = Vec::new();
		let mut name = None;
		let mut property_keys = Vec::new();
		let mut property_values = Vec::new();
		let mut version = 1;

		while reader.has_remaining() {
			match reader.read_pbf_key()? {
				(1, 2) => name = Some(reader.read_pbf_string()?),
				(2, 2) => features.push(VectorTileFeature::read(&mut reader.get_pbf_sub_reader()?)?),
				(3, 2) => property_keys.push(reader.read_pbf_string()?),
				(4, 2) => property_values.push(GeoValue::read(&mut reader.get_pbf_sub_reader()?)?),
				(5, 0) => extent = reader.read_varint()? as u32,
				(15, 0) => version = reader.read_varint()? as u32,
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		Ok(VectorTileLayer {
			extent,
			features,
			name: name.ok_or(anyhow!("Layer name is required"))?,
			property_keys,
			property_values,
			version,
		})
	}

	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = BlobWriter::new_le();

		writer.write_pbf_key(1, 2)?;
		writer.write_pbf_string(&self.name)?;

		for feature in self.features.iter() {
			writer.write_pbf_key(2, 2)?;
			writer.write_pbf_blob(&feature.to_blob()?)?;
		}

		for key in self.property_keys.iter() {
			writer.write_pbf_key(3, 2)?;
			writer.write_pbf_string(&key)?;
		}

		for value in self.property_values.iter() {
			writer.write_pbf_key(4, 2)?;
			writer.write_pbf_blob(&value.to_blob()?)?;
		}

		if self.extent != 4096 {
			writer.write_pbf_key(5, 0)?;
			writer.write_varint(self.extent as u64)?;
		}

		if self.version != 1 {
			writer.write_pbf_key(15, 0)?;
			writer.write_varint(self.version as u64)?;
		}

		Ok(writer.into_blob())
	}

	pub fn translate_tag_ids(&self, tag_ids: &[u32]) -> Result<GeoProperties> {
		ensure!(tag_ids.len() % 2 == 0, "must be even");
		let mut attributes = GeoProperties::new();
		for i in 0..tag_ids.len().div(2) {
			let tag_key = tag_ids[i * 2] as usize;
			let tag_val = tag_ids[i * 2 + 1] as usize;
			attributes.insert(
				self
					.property_keys
					.get(tag_key)
					.ok_or(anyhow!("key not found"))?
					.to_owned(),
				self
					.property_values
					.get(tag_val)
					.ok_or(anyhow!("value not found"))?
					.clone(),
			);
		}
		Ok(attributes)
	}
}
