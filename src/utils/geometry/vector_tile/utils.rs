use anyhow::{bail, Result};

pub fn parse_key(data: &[u8]) -> Result<(u32, u8, usize)> {
	let (key, read_bytes) = parse_varint(data)?;
	let field_number = (key >> 3) as u32;
	let wire_type = (key & 0x07) as u8;
	Ok((field_number, wire_type, read_bytes))
}

pub fn parse_varint(data: &[u8]) -> Result<(u64, usize)> {
	let mut result = 0u64;
	let mut shift = 0;
	for (i, byte) in data.iter().enumerate() {
		let byte_val = *byte as u64;
		result |= (byte_val & 0x7F) << shift;
		if byte_val & 0x80 == 0 {
			return Ok((result, i + 1));
		}
		shift += 7;
	}
	bail!("Varint parsing error")
}

pub fn parse_packed_uint32(data: &[u8]) -> Result<Vec<u32>> {
	let mut values = Vec::new();
	let mut i = 0;
	while i < data.len() {
		let (value, read_bytes) = parse_varint(&data[i..])?;
		i += read_bytes;
		values.push(value as u32);
	}
	Ok(values)
}
