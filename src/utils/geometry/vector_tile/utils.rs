#![allow(dead_code)]

use crate::utils::{BlobReader, BlobWriter};
use anyhow::Result;
use byteorder::LE;

pub fn parse_key(key: u64) -> (u32, u8) {
	((key >> 3) as u32, (key & 0x07) as u8)
}

pub fn compose_key(field_number: u32, wire_type: u8) -> u64 {
	((field_number as u64) << 3) | (wire_type as u64)
}

pub fn parse_packed_uint32(reader: &mut BlobReader<LE>) -> Result<Vec<u32>> {
	let mut values = Vec::new();
	while reader.has_remaining() {
		values.push(reader.read_varint()? as u32);
	}
	Ok(values)
}

pub fn write_packed_uint32(writer: &mut BlobWriter<LE>, values: &[u32]) -> Result<()> {
	for &value in values {
		writer.write_varint(value as u64)?;
	}
	Ok(())
}
