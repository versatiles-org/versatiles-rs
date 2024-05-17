use super::{attributes::AttributeLookup, parse_key, parse_packed_uint32, GeomType};
use crate::utils::{
	geometry::types::{
		LinestringGeometry, MultiLinestringFeature, MultiLinestringGeometry, MultiPointFeature, MultiPolygonFeature,
		MultiPolygonGeometry, PointGeometry, PolygonGeometry, Ring,
	},
	BlobReader,
};
use anyhow::{bail, ensure, Result};
use byteorder::LE;

#[derive(Debug, PartialEq)]
pub struct VectorTileFeature {
	pub id: Option<u64>,
	pub tag_ids: Vec<u32>,
	pub geometry_type: GeomType,
	pub linestrings: MultiLinestringGeometry,
}

impl VectorTileFeature {
	pub fn decode(reader: &mut BlobReader<LE>) -> Result<VectorTileFeature> {
		let mut id: Option<u64> = None;
		let mut tag_ids: Vec<u32> = Vec::new();
		let mut geometry_type = GeomType::Unknown;
		let mut geometry: MultiLinestringGeometry = Vec::new();

		while reader.has_remaining() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);
			let value = reader.read_varint()?;

			match (field_number, wire_type) {
				(1, 0) => id = Some(value),
				(2, 2) => tag_ids = parse_packed_uint32(&mut reader.get_sub_reader(value)?)?,
				(3, 0) => geometry_type = GeomType::from(value),
				(4, 2) => geometry = decode_geometry(&mut reader.get_sub_reader(value)?)?,
				_ => bail!("Unexpected field number or wire type: ({field_number}, {wire_type})"),
			}
		}

		Ok(VectorTileFeature {
			id,
			tag_ids,
			geometry_type,
			linestrings: geometry,
		})
	}

	pub fn into_multi_point_feature(mut self, attributes: &AttributeLookup) -> Result<MultiPointFeature> {
		ensure!(self.linestrings.len() == 1, "(Multi)Points must have exactly one entry");
		let geometry = self.linestrings.pop().unwrap();
		ensure!(!geometry.is_empty(), "The entry in (Multi)Points must not be empty");

		Ok(MultiPointFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry,
		})
	}

	pub fn into_multi_linestring_feature(self, attributes: &AttributeLookup) -> Result<MultiLinestringFeature> {
		ensure!(
			!self.linestrings.is_empty(),
			"MultiLinestrings must have at least one entry"
		);
		for line in &self.linestrings {
			ensure!(
				line.len() >= 2,
				"Each entry in MultiLinestrings must have at least two points"
			);
		}

		Ok(MultiLinestringFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry: self.linestrings,
		})
	}

	pub fn into_multi_polygon_feature(self, attributes: &AttributeLookup) -> Result<MultiPolygonFeature> {
		ensure!(!self.linestrings.is_empty(), "Polygons must have at least one entry");
		let mut current_polygon: PolygonGeometry = Vec::new();
		let mut geometry: MultiPolygonGeometry = Vec::new();

		for ring in self.linestrings {
			ensure!(
				ring.len() >= 4,
				"Each ring in Polygons must have at least four points (A,B,C,A)"
			);

			ensure!(
				ring[0] == ring[ring.len() - 1],
				"First and last point of the ring must be the same"
			);

			let area = ring.area();

			if area > 1e-10 {
				// Outer ring
				if !current_polygon.is_empty() {
					geometry.push(current_polygon);
					current_polygon = Vec::new();
				}
				current_polygon.push(ring);
			} else if area < -1e-10 {
				// Inner ring
				ensure!(!current_polygon.is_empty(), "An outer ring must precede inner rings");
				current_polygon.push(ring);
			} else {
				bail!("Error: Ring with zero area")
			}
		}

		if !current_polygon.is_empty() {
			geometry.push(current_polygon);
		}

		Ok(MultiPolygonFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry,
		})
	}
}

fn decode_geometry(reader: &mut BlobReader<LE>) -> Result<MultiLinestringGeometry> {
	// https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding

	let mut linestrings: MultiLinestringGeometry = Vec::new();
	let mut current_line: LinestringGeometry = Vec::new();
	let mut x = 0;
	let mut y = 0;

	while reader.has_remaining() {
		let value = reader.read_varint()?;
		let command = value & 0x7;
		let count = value >> 3;

		match command {
			1 | 2 => {
				// MoveTo or LineTo command
				if command == 1 && !current_line.is_empty() {
					// MoveTo command indicates the start of a new linestring
					linestrings.push(current_line);
					current_line = Vec::new();
				}

				for _ in 0..count {
					x += reader.read_svarint()?;
					y += reader.read_svarint()?;
					current_line.push(PointGeometry {
						x: x as f64,
						y: y as f64,
					});
				}
			}
			7 => {
				// ClosePath command
				ensure!(
					!current_line.is_empty(),
					"ClosePath command found on an empty linestring"
				);
				current_line.push(current_line[0].clone());
			}
			_ => bail!("Unknown command {}", command),
		}
	}

	if !current_line.is_empty() {
		linestrings.push(current_line);
	}

	Ok(linestrings)
}
