use super::{parse_key, parse_varint, Feature, Value};
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
	pub fn decode(data: &[u8]) -> Result<Layer> {
		let mut layer = Layer::default();
		let mut i = 0;
		while i < data.len() {
			let (field_number, wire_type, read_bytes) = parse_key(&data[i..])?;
			i += read_bytes;

			match (field_number, wire_type) {
				(1, 2) => {
					let (len, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					let name_data = &data[i..i + len as usize];
					i += len as usize;
					layer.name = Some(String::from_utf8(name_data.to_vec())?);
				}
				(2, 2) => {
					let (len, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					let feature_data = &data[i..i + len as usize];
					i += len as usize;
					let feature = Feature::decode(feature_data)?;
					layer.features.push(feature);
				}
				(3, 2) => {
					let (len, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					let key_data = &data[i..i + len as usize];
					i += len as usize;
					layer.keys.push(String::from_utf8(key_data.to_vec())?);
				}
				(4, 2) => {
					let (len, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					let value_data = &data[i..i + len as usize];
					i += len as usize;
					let value = Value::decode(value_data)?;
					layer.values.push(value);
				}
				(5, 0) => {
					let (extent, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					layer.extent = Some(extent as u32);
				}
				(15, 0) => {
					let (version, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					layer.version = Some(version as u32);
				}
				_ => bail!("Unexpected field number or wire type"),
			}
		}
		Ok(layer)
	}
}
