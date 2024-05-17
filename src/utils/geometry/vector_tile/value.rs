use super::parse_key;
use crate::utils::{geometry::types::GeoValue, BlobReader};
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
