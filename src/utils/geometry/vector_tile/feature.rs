use super::{parse_key, parse_packed_uint32, parse_varint, GeomType};
use anyhow::{bail, Result};

#[derive(Debug, Default, PartialEq)]
pub struct Feature {
	pub id: Option<u64>,
	pub tags: Vec<u32>,
	pub geom_type: Option<GeomType>,
	pub geometry: Vec<u32>,
}

impl Feature {
	pub fn decode(data: &[u8]) -> Result<Feature> {
		let mut feature = Feature::default();
		let mut i = 0;
		while i < data.len() {
			let (field_number, wire_type, read_bytes) = parse_key(&data[i..])?;
			i += read_bytes;

			match (field_number, wire_type) {
				(1, 0) => {
					let (id, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					feature.id = Some(id as u64);
				}
				(2, 2) => {
					let (len, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					let tags_data = &data[i..i + len as usize];
					i += len as usize;
					feature.tags = parse_packed_uint32(tags_data)?;
				}
				(3, 0) => {
					let (type_, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					feature.geom_type = Some(GeomType::from_i32(type_ as i32));
				}
				(4, 2) => {
					let (len, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					let geometry_data = &data[i..i + len as usize];
					i += len as usize;
					feature.geometry = parse_packed_uint32(geometry_data)?;
				}
				_ => bail!("Unexpected field number or wire type"),
			}
		}
		Ok(feature)
	}
}
