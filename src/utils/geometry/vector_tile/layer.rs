use super::{attributes::AttributeLookup, decode_value, parse_key, VectorTileFeature};
use crate::utils::{
	geometry::types::{MultiLinestringFeature, MultiPointFeature, MultiPolygonFeature},
	BlobReader,
};
use anyhow::{bail, Result};
use byteorder::LE;

#[derive(Debug, Default, PartialEq)]
pub struct VectorTileLayer {
	pub version: u32,
	pub name: String,
	pub points: Vec<MultiPointFeature>,
	pub line_strings: Vec<MultiLinestringFeature>,
	pub polygons: Vec<MultiPolygonFeature>,
	pub extent: u32,
}

impl VectorTileLayer {
	pub fn decode(reader: &mut BlobReader<LE>) -> Result<VectorTileLayer> {
		let mut attributes = AttributeLookup::new();
		let mut features: Vec<VectorTileFeature> = Vec::new();
		let mut version = 0;
		let mut name = String::new();
		let mut points: Vec<MultiPointFeature> = Vec::new();
		let mut line_strings: Vec<MultiLinestringFeature> = Vec::new();
		let mut polygons: Vec<MultiPolygonFeature> = Vec::new();
		let mut extent = 4096;

		while reader.has_remaining() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);
			let value = reader.read_varint()?;
			match (field_number, wire_type) {
				(1, 2) => name = reader.read_string(value)?,
				(2, 2) => features.push(VectorTileFeature::decode(&mut reader.get_sub_reader(value)?)?),
				(3, 2) => attributes.add_key(reader.read_string(value)?),
				(4, 2) => attributes.add_value(decode_value(&mut reader.get_sub_reader(value)?)?),
				(5, 0) => extent = value as u32,
				(15, 0) => version = value as u32,
				_ => bail!("Unexpected field number or wire type"),
			}
		}

		for feature in features {
			match feature.geom_type {
				super::GeomType::Unknown => (),
				super::GeomType::Point => points.push(feature.to_multi_point_feature(&attributes)?),
				super::GeomType::Linestring => line_strings.push(feature.to_multi_linestring_feature(&attributes)?),
				super::GeomType::Polygon => polygons.push(feature.to_multi_polygon_feature(&attributes)?),
			}
		}

		Ok(VectorTileLayer {
			version,
			name,
			points,
			line_strings,
			polygons,
			extent,
		})
	}
}
