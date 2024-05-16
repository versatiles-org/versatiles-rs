use crate::{types::Blob, utils::BlobReader};
use anyhow::Result;

pub fn parse_key(key: u64) -> (u32, u8) {
	let field_number = (key >> 3) as u32;
	let wire_type = (key & 0x07) as u8;
	(field_number, wire_type)
}

pub fn parse_packed_uint32(blob: &Blob) -> Result<Vec<u32>> {
	let mut reader = BlobReader::new_le(blob);
	let mut values = Vec::new();
	while reader.data_left() {
		values.push(reader.read_varint()? as u32);
	}
	Ok(values)
}
