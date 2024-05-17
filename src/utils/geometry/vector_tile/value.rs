#![allow(dead_code)]

use super::{compose_key, parse_key};
use crate::utils::{geometry::types::GeoValue, BlobReader, BlobWriter};
use anyhow::{anyhow, bail, Result};
use byteorder::LE;

pub fn decode_value(reader: &mut BlobReader<LE>) -> Result<GeoValue> {
	// source: https://protobuf.dev/programming-guides/encoding/

	use GeoValue::*;
	let mut value: Option<GeoValue> = None;

	while reader.has_remaining() {
		let (field_number, wire_type) = parse_key(reader.read_varint()?);

		value = Some(match (field_number, wire_type) {
			// https://protobuf.dev/programming-guides/encoding/#structure
			(1, 2) => {
				let len = reader.read_varint()?;
				GeoString(reader.read_string(len)?)
			}
			(2, 5) => GeoF32(reader.read_f32()?),
			(3, 1) => GeoF64(reader.read_f64()?),
			(4, 0) => GeoI64(reader.read_varint()? as i64),
			(5, 0) => GeoU64(reader.read_varint()?),
			(6, 0) => GeoI64(reader.read_svarint()?),
			(7, 0) => GeoBool(reader.read_varint()? != 0),
			_ => bail!("Unexpected field number or wire type".to_string()),
		})
	}
	value.ok_or(anyhow!("no value found"))
}

pub fn encode_value(writer: &mut BlobWriter<LE>, value: GeoValue) -> Result<()> {
	match value {
		GeoValue::GeoString(s) => {
			writer.write_varint(compose_key(1, 2))?;
			writer.write_varint(s.len() as u64)?;
			writer.write_string(&s)?;
		}
		GeoValue::GeoF32(f) => {
			writer.write_varint(compose_key(2, 5))?;
			writer.write_f32(f)?;
		}
		GeoValue::GeoF64(f) => {
			writer.write_varint(compose_key(3, 1))?;
			writer.write_f64(f)?;
		}
		GeoValue::GeoU64(u) => {
			writer.write_varint(compose_key(5, 0))?;
			writer.write_varint(u)?;
		}
		GeoValue::GeoI64(s) => {
			writer.write_varint(compose_key(6, 0))?;
			writer.write_svarint(s)?;
		}
		GeoValue::GeoBool(b) => {
			writer.write_varint(compose_key(7, 0))?;
			writer.write_varint(if b { 1 } else { 0 })?;
		}
	}

	Ok(())
}
