use crate::utils::BlobReader;
use anyhow::Result;
use byteorder::LE;

pub fn parse_key(key: u64) -> (u32, u8) {
	let field_number = (key >> 3) as u32;
	let wire_type = (key & 0x07) as u8;
	(field_number, wire_type)
}

pub fn parse_packed_uint32(reader: &mut BlobReader<LE>) -> Result<Vec<u32>> {
	let mut values = Vec::new();
	while reader.has_remaining() {
		values.push(reader.read_varint()? as u32);
	}
	Ok(values)
}
