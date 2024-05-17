use super::{attributes::AttributeLookup, decode_value, parse_key, VTFeature};
use crate::utils::{
	geometry::types::{MultiLinestringFeature, MultiPointFeature, MultiPolygonFeature},
	BlobReader,
};
use anyhow::{bail, Result};
use byteorder::LE;

#[derive(Debug, Default, PartialEq)]
pub struct Layer {
	pub version: Option<u32>,
	pub name: Option<String>,
	pub points: Vec<MultiPointFeature>,
	pub line_strings: Vec<MultiLinestringFeature>,
	pub polygons: Vec<MultiPolygonFeature>,
	pub extent: Option<u32>,
}

impl Layer {
	pub fn decode(reader: &mut BlobReader<LE>) -> Result<Layer> {
		let mut attributes = AttributeLookup::new();
		let mut features: Vec<VTFeature> = Vec::new();
		let mut version: Option<u32> = None;
		let mut name: Option<String> = None;
		let mut points: Vec<MultiPointFeature> = Vec::new();
		let mut line_strings: Vec<MultiLinestringFeature> = Vec::new();
		let mut polygons: Vec<MultiPolygonFeature> = Vec::new();
		let mut extent: Option<u32> = None;

		while reader.has_remaining() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);
			let value = reader.read_varint()?;
			match (field_number, wire_type) {
				(1, 2) => name = Some(reader.read_string(value)?),
				(2, 2) => features.push(VTFeature::decode(&mut reader.get_sub_reader(value)?)?),
				(3, 2) => attributes.add_key(reader.read_string(value)?),
				(4, 2) => attributes.add_value(decode_value(&mut reader.get_sub_reader(value)?)?),
				(5, 0) => extent = Some(value as u32),
				(15, 0) => version = Some(value as u32),
				_ => bail!("Unexpected field number or wire type"),
			}
		}

		for feature in features {
			match feature.geom_type {
				super::GeomType::Unknown => (),
				super::GeomType::Point => points.push(feature.into_points(&attributes)?),
				super::GeomType::Linestring => line_strings.push(feature.into_linestrings(&attributes)?),
				super::GeomType::Polygon => polygons.push(feature.into_polygons(&attributes)?),
			}
		}

		Ok(Layer {
			version,
			name,
			points,
			line_strings,
			polygons,
			extent,
		})
	}
}
