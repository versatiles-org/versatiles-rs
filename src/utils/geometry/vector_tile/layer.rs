use super::{parse_key, Feature, Value};
use crate::{types::Blob, utils::BlobReader};
use anyhow::{bail, Result};

#[derive(Debug, Default, PartialEq)]
pub struct Layer {
	pub version: Option<u32>,
	pub name: Option<String>,
	pub features: Vec<Feature>,
	pub keys: Vec<String>,
	pub values: Vec<Value>,
	pub extent: Option<u32>,
}

impl Layer {
	pub fn decode(blob: &Blob) -> Result<Layer> {
		let mut reader = BlobReader::new_le(blob);
		let mut layer = Layer::default();
		while reader.data_left() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);
			let value = reader.read_varint()?;
			match (field_number, wire_type) {
				(1, 2) => {
					layer.name = Some(reader.read_string(value)?);
				}
				(2, 2) => {
					layer.features.push(Feature::decode(&reader.read_blob(value)?)?);
				}
				(3, 2) => {
					layer.keys.push(reader.read_string(value)?);
				}
				(4, 2) => {
					layer.values.push(Value::decode(&reader.read_blob(value)?)?);
				}
				(5, 0) => {
					layer.extent = Some(value as u32);
				}
				(15, 0) => {
					layer.version = Some(value as u32);
				}
				_ => bail!("Unexpected field number or wire type"),
			}
		}
		Ok(layer)
	}
}
