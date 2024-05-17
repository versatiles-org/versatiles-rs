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
pub struct VTFeature {
	pub id: Option<u64>,
	pub tag_ids: Vec<u32>,
	pub geom_type: GeomType,
	pub lines: MultiLinestringGeometry,
}

impl VTFeature {
	pub fn decode(reader: &mut BlobReader<LE>) -> Result<VTFeature> {
		let mut id: Option<u64> = None;
		let mut tags: Vec<u32> = Vec::new();
		let mut geom_type: GeomType = GeomType::Unknown;
		let mut geometry: MultiLinestringGeometry = Vec::new();

		while reader.has_remaining() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);
			let value = reader.read_varint()?;

			match (field_number, wire_type) {
				(1, 0) => id = Some(value),
				(2, 2) => tags = parse_packed_uint32(&mut reader.get_sub_reader(value)?)?,
				(3, 0) => geom_type = GeomType::from(value),
				(4, 2) => geometry = parse_geometry(&mut reader.get_sub_reader(value)?)?,
				_ => bail!("Unexpected field number or wire type"),
			}
		}
		Ok(VTFeature {
			id,
			tag_ids: tags,
			geom_type,
			lines: geometry,
		})
	}

	pub fn into_points(mut self, attributes: &AttributeLookup) -> Result<MultiPointFeature> {
		ensure!(self.lines.len() == 1, "(Multi)Points must have exactly one entry");
		let geometry = self.lines.pop().unwrap();
		ensure!(!geometry.is_empty(), "The entry in (Multi)Points must not be empty");

		Ok(MultiPointFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry,
		})
	}

	pub fn into_linestrings(self, attributes: &AttributeLookup) -> Result<MultiLinestringFeature> {
		ensure!(!self.lines.is_empty(), "MultiLinestrings must have at least one entry");
		for line in self.lines.iter() {
			ensure!(
				line.len() >= 2,
				"Each entry in MultiLinestrings must have at least two points"
			);
		}

		Ok(MultiLinestringFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry: self.lines,
		})
	}

	pub fn into_polygons(self, attributes: &AttributeLookup) -> Result<MultiPolygonFeature> {
		ensure!(!self.lines.is_empty(), "Polygons must have at least one entry");
		let mut polygon: PolygonGeometry = Vec::new();
		let mut geometry: MultiPolygonGeometry = Vec::new();

		for ring in self.lines.into_iter() {
			ensure!(
				ring.len() >= 4,
				"Each entry in Polygons must have at least four points (A,B,C,A)"
			);

			ensure!(ring[0] == ring[ring.len() - 1], "First and last point must be the same");

			let area = ring.area();

			if area > 1e-10 {
				//outer ring
				if !polygon.is_empty() {
					geometry.push(polygon);
					polygon = Vec::new();
				}
				polygon.push(ring);
			} else if area < -1e-10 {
				// inner ring
				ensure!(!polygon.is_empty(), "there must already be an outer ring");
				polygon.push(ring);
			} else {
				bail!("error")
			}
		}

		if !polygon.is_empty() {
			geometry.push(polygon);
		}

		Ok(MultiPolygonFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry,
		})
	}
}

fn parse_geometry(reader: &mut BlobReader<LE>) -> Result<MultiLinestringGeometry> {
	// https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding

	let mut lines: MultiLinestringGeometry = Vec::new();
	let mut line: LinestringGeometry = Vec::new();
	let mut x = 0;
	let mut y = 0;

	while reader.has_remaining() {
		let v = reader.read_varint()?;
		let cmd = v & 0x7;
		let count = v >> 3;

		match cmd {
			1 | 2 => {
				// move to / line to
				if cmd == 1 {
					// move to
					if !line.is_empty() {
						lines.push(line)
					};
					line = Vec::new();
				}

				for _ in 0..count {
					x += reader.read_svarint()?;
					y += reader.read_svarint()?;
					line.push(PointGeometry {
						x: x as f64,
						y: y as f64,
					});
				}
			}
			7 => {
				// close path
				ensure!(!line.is_empty());
				line.push(line[0].clone());
			}
			_ => bail!("unknown command {cmd}"),
		}
	}

	if !line.is_empty() {
		lines.push(line)
	};

	Ok(lines)
}
