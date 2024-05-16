use super::parse_key;
use crate::utils::BlobReader;
use anyhow::{bail, Result};
use byteorder::LE;

#[derive(Debug, Default, PartialEq)]
pub struct Value {
	pub string_value: Option<String>,
	pub float_value: Option<f32>,
	pub double_value: Option<f64>,
	pub int_value: Option<i64>,
	pub uint_value: Option<u64>,
	pub sint_value: Option<i64>,
	pub bool_value: Option<bool>,
}

impl Value {
	pub fn decode(reader: &mut BlobReader<LE>) -> Result<Value> {
		let mut value = Value::default();

		while reader.has_remaining() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);

			match (field_number, wire_type) {
				(1, 2) => {
					let len = reader.read_varint()?;
					value.string_value = Some(reader.read_string(len)?);
				}
				(2, 5) => {
					value.float_value = Some(reader.read_f32()?);
				}
				(3, 1) => {
					value.double_value = Some(reader.read_f64()?);
				}
				(4, 0) => {
					value.int_value = Some(reader.read_varint()? as i64);
				}
				(5, 0) => {
					value.uint_value = Some(reader.read_varint()?);
				}
				(6, 0) => {
					let sint_value = reader.read_varint()?;
					value.sint_value = Some((sint_value >> 1) as i64 ^ -((sint_value & 1) as i64));
				}
				(7, 0) => {
					let bool_value = reader.read_varint()?;
					value.bool_value = Some(bool_value != 0);
				}
				_ => bail!("Unexpected field number or wire type".to_string()),
			}
		}
		Ok(value)
	}
}
