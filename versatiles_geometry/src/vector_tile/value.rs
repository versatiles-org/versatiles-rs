#![allow(dead_code)]

use crate::{
	geometry::GeoValue,
	io::{ValueReader, ValueWriter, ValueWriterBlob},
	types::Blob,
};
use anyhow::{anyhow, bail, Context, Result};
use byteorder::LE;

pub trait GeoValuePBF<'a> {
	fn read(reader: &mut dyn ValueReader<'a, LE>) -> Result<GeoValue>;
	fn to_blob(&self) -> Result<Blob>;
}

impl<'a> GeoValuePBF<'a> for GeoValue {
	fn read(reader: &mut dyn ValueReader<'a, LE>) -> Result<GeoValue> {
		// source: https://protobuf.dev/programming-guides/encoding/

		use GeoValue::*;
		let mut value: Option<GeoValue> = None;

		while reader.has_remaining() {
			value = Some(
				match reader.read_pbf_key().context("Failed to read PBF key")? {
					// https://protobuf.dev/programming-guides/encoding/#structure
					(1, 2) => {
						let len = reader
							.read_varint()
							.context("Failed to read varint for string length")?;
						String(
							reader
								.read_string(len)
								.context("Failed to read string value")?,
						)
					}
					(2, 5) => Float(reader.read_f32().context("Failed to read f32 value")?),
					(3, 1) => Double(reader.read_f64().context("Failed to read f64 value")?),
					(4, 0) => Int(
						reader
							.read_varint()
							.context("Failed to read varint for int value")? as i64,
					),
					(5, 0) => UInt(
						reader
							.read_varint()
							.context("Failed to read varint for uint value")?,
					),
					(6, 0) => Int(
						reader
							.read_svarint()
							.context("Failed to read svarint value")?,
					),
					(7, 0) => Bool(
						reader
							.read_varint()
							.context("Failed to read varint for bool value")?
							!= 0,
					),
					(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
				},
			)
		}
		value
			.ok_or_else(|| anyhow!("No value found"))
			.context("Failed to read GeoValue")
	}

	fn to_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_le();

		match self {
			GeoValue::String(s) => {
				writer
					.write_pbf_key(1, 2)
					.context("Failed to write PBF key for string value")?;
				writer
					.write_pbf_string(s)
					.context("Failed to write string value")?;
			}
			GeoValue::Float(f) => {
				writer
					.write_pbf_key(2, 5)
					.context("Failed to write PBF key for float value")?;
				writer
					.write_f32(*f)
					.context("Failed to write float value")?;
			}
			GeoValue::Double(f) => {
				writer
					.write_pbf_key(3, 1)
					.context("Failed to write PBF key for double value")?;
				writer
					.write_f64(*f)
					.context("Failed to write double value")?;
			}
			GeoValue::UInt(u) => {
				writer
					.write_pbf_key(5, 0)
					.context("Failed to write PBF key for uint value")?;
				writer
					.write_varint(*u)
					.context("Failed to write uint value")?;
			}
			GeoValue::Int(s) => {
				writer
					.write_pbf_key(6, 0)
					.context("Failed to write PBF key for int value")?;
				writer
					.write_svarint(*s)
					.context("Failed to write int value")?;
			}
			GeoValue::Bool(b) => {
				writer
					.write_pbf_key(7, 0)
					.context("Failed to write PBF key for bool value")?;
				writer
					.write_varint(if *b { 1 } else { 0 })
					.context("Failed to write bool value")?;
			}
		}

		Ok(writer.into_blob())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::io::ValueReaderSlice;

	#[test]
	fn test_read_string() -> Result<()> {
		let data = vec![
			0x0A, // PBF key (field number 1, wire type 2)
			0x05, // Length of the string
			b'h', b'e', b'l', b'l', b'o', // The string "hello"
		];
		let mut reader = ValueReaderSlice::new_le(&data);
		let geo_value = GeoValue::read(&mut reader)?;
		assert_eq!(geo_value, GeoValue::from("hello"));
		Ok(())
	}

	#[test]
	fn test_to_blob_string() -> Result<()> {
		let geo_value = GeoValue::from("hello");
		let blob = geo_value.to_blob()?;
		let expected = vec![
			0x0A, // PBF key (field number 1, wire type 2)
			0x05, // Length of the string
			b'h', b'e', b'l', b'l', b'o', // The string "hello"
		];
		assert_eq!(blob.into_vec(), expected);
		Ok(())
	}

	#[test]
	fn test_read_float() -> Result<()> {
		let data = vec![
			0x15, // PBF key (field number 2, wire type 5)
			0x00, 0x00, 0x80, 0x3F, // The float value 1.0
		];
		let mut reader = ValueReaderSlice::new_le(&data);
		let geo_value = GeoValue::read(&mut reader)?;
		assert_eq!(geo_value, GeoValue::Float(1.0));
		Ok(())
	}

	#[test]
	fn test_to_blob_float() -> Result<()> {
		let geo_value = GeoValue::Float(1.0);
		let blob = geo_value.to_blob()?;
		let expected = vec![
			0x15, // PBF key (field number 2, wire type 5)
			0x00, 0x00, 0x80, 0x3F, // The float value 1.0
		];
		assert_eq!(blob.into_vec(), expected);
		Ok(())
	}

	#[test]
	fn test_read_double() -> Result<()> {
		let data = vec![
			0x19, // PBF key (field number 3, wire type 1)
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // The double value 1.0
		];
		let mut reader = ValueReaderSlice::new_le(&data);
		let geo_value = GeoValue::read(&mut reader)?;
		assert_eq!(geo_value, GeoValue::Double(1.0));
		Ok(())
	}

	#[test]
	fn test_to_blob_double() -> Result<()> {
		let geo_value = GeoValue::Double(1.0);
		let blob = geo_value.to_blob()?;
		let expected = vec![
			0x19, // PBF key (field number 3, wire type 1)
			0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // The double value 1.0
		];
		assert_eq!(blob.into_vec(), expected);
		Ok(())
	}

	#[test]
	fn test_read_int() -> Result<()> {
		let data = vec![
			0x30, // PBF key (field number 6, wire type 0)
			0x96, 0x01, // The varint value 150
		];
		let mut reader = ValueReaderSlice::new_le(&data);
		let geo_value = GeoValue::read(&mut reader)?;
		assert_eq!(geo_value, GeoValue::Int(75));
		Ok(())
	}

	#[test]
	fn test_to_blob_int() -> Result<()> {
		let geo_value = GeoValue::Int(75);
		let blob = geo_value.to_blob()?;
		let expected = vec![
			0x30, // PBF key (field number 6, wire type 0)
			0x96, 0x01, // The varint value 150
		];
		assert_eq!(blob.into_vec(), expected);
		Ok(())
	}

	#[test]
	fn test_read_uint() -> Result<()> {
		let data = vec![
			0x28, // PBF key (field number 5, wire type 0)
			0x96, 0x01, // The varint value 150
		];
		let mut reader = ValueReaderSlice::new_le(&data);
		let geo_value = GeoValue::read(&mut reader)?;
		assert_eq!(geo_value, GeoValue::UInt(150));
		Ok(())
	}

	#[test]
	fn test_to_blob_uint() -> Result<()> {
		let geo_value = GeoValue::UInt(150);
		let blob = geo_value.to_blob()?;
		let expected = vec![
			0x28, // PBF key (field number 5, wire type 0)
			0x96, 0x01, // The varint value 150
		];
		assert_eq!(blob.into_vec(), expected);
		Ok(())
	}

	#[test]
	fn test_read_bool() -> Result<()> {
		let data = vec![
			0x38, // PBF key (field number 7, wire type 0)
			0x01, // The varint value 1 (true)
		];
		let mut reader = ValueReaderSlice::new_le(&data);
		let geo_value = GeoValue::read(&mut reader)?;
		assert_eq!(geo_value, GeoValue::Bool(true));
		Ok(())
	}

	#[test]
	fn test_to_blob_bool() -> Result<()> {
		let geo_value = GeoValue::Bool(true);
		let blob = geo_value.to_blob()?;
		let expected = vec![
			0x38, // PBF key (field number 7, wire type 0)
			0x01, // The varint value 1 (true)
		];
		assert_eq!(blob.into_vec(), expected);
		Ok(())
	}
}
