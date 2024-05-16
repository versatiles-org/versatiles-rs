use super::{parse_key, parse_packed_uint32, GeomType};
use crate::utils::BlobReader;
use anyhow::{bail, Result};
use byteorder::LE;

#[derive(Debug, Default, PartialEq)]
pub struct Feature {
	pub id: Option<u64>,
	pub tags: Vec<u32>,
	pub geom_type: Option<GeomType>,
	pub geometry: Vec<u32>,
}

impl Feature {
	pub fn decode(reader: &mut BlobReader<LE>) -> Result<Feature> {
		let mut id: Option<u64> = None;
		let mut tags: Vec<u32> = Vec::new();
		let mut geom_type: Option<GeomType> = None;
		let mut geometry: Vec<u32> = Vec::new();

		while reader.has_remaining() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);
			let value = reader.read_varint()?;

			match (field_number, wire_type) {
				(1, 0) => {
					id = Some(value);
				}
				(2, 2) => {
					tags = parse_packed_uint32(&mut reader.get_sub_reader(value)?)?;
				}
				(3, 0) => {
					geom_type = Some(GeomType::from(value));
				}
				(4, 2) => {
					geometry = parse_packed_uint32(&mut reader.get_sub_reader(value)?)?;
				}
				_ => bail!("Unexpected field number or wire type"),
			}
		}
		Ok(Feature {
			id,
			tags,
			geom_type,
			geometry,
		})
	}
}
