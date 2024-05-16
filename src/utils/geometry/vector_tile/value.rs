use super::{parse_key, parse_varint};
use anyhow::{bail, Result};

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
	pub fn decode(data: &[u8]) -> Result<Value> {
		let mut value = Value::default();
		let mut i = 0;
		while i < data.len() {
			let (field_number, wire_type, read_bytes) = parse_key(&data[i..])?;
			i += read_bytes;

			match (field_number, wire_type) {
				(1, 2) => {
					let (len, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					let string_data = &data[i..i + len as usize];
					i += len as usize;
					value.string_value = Some(String::from_utf8(string_data.to_vec())?);
				}
				(2, 5) => {
					value.float_value = Some(f32::from_le_bytes(data[i..i + 4].try_into()?));
					i += 4;
				}
				(3, 1) => {
					value.double_value = Some(f64::from_le_bytes(data[i..i + 8].try_into()?));
					i += 8;
				}
				(4, 0) => {
					let (int_value, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					value.int_value = Some(int_value as i64);
				}
				(5, 0) => {
					let (uint_value, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					value.uint_value = Some(uint_value as u64);
				}
				(6, 0) => {
					let (sint_value, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					value.sint_value = Some((sint_value >> 1) as i64 ^ -((sint_value & 1) as i64));
				}
				(7, 0) => {
					let (bool_value, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					value.bool_value = Some(bool_value != 0);
				}
				_ => bail!("Unexpected field number or wire type".to_string()),
			}
		}
		Ok(value)
	}
}
