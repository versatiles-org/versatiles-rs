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
	pub geom_type: GeomType,
	pub linestrings: MultiLinestringGeometry,
}

impl VectorTileFeature {
	/// Decodes a `VectorTileFeature` from a `BlobReader`.
	pub fn decode(reader: &mut BlobReader<LE>) -> Result<VectorTileFeature> {
		let mut feature_id: Option<u64> = None;
		let mut tag_ids: Vec<u32> = Vec::new();
		let mut geometry_type: GeomType = GeomType::Unknown;
		let mut linestrings: MultiLinestringGeometry = Vec::new();

		while reader.has_remaining() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);
			let value = reader.read_varint()?;

			match (field_number, wire_type) {
				(1, 0) => feature_id = Some(value),
				(2, 2) => tag_ids = parse_packed_uint32(&mut reader.get_sub_reader(value)?)?,
				(3, 0) => geometry_type = GeomType::from(value),
				(4, 2) => linestrings = decode_linestring_geometry(&mut reader.get_sub_reader(value)?)?,
				_ => bail!("Unexpected field number or wire type: ({field_number}, {wire_type})"),
			}
		}

		Ok(VectorTileFeature {
			id: feature_id,
			tag_ids,
			geom_type: geometry_type,
			linestrings,
		})
	}

	/// Converts the `VectorTileFeature` into a `MultiPointFeature`.
	pub fn to_multi_point_feature(mut self, attributes: &AttributeLookup) -> Result<MultiPointFeature> {
		ensure!(self.linestrings.len() == 1, "(Multi)Points must have exactly one entry");
		let geometry = self.linestrings.pop().unwrap();
		ensure!(!geometry.is_empty(), "The entry in (Multi)Points must not be empty");

		Ok(MultiPointFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry,
		})
	}

	/// Converts the `VectorTileFeature` into a `MultiLinestringFeature`.
	pub fn to_multi_linestring_feature(self, attributes: &AttributeLookup) -> Result<MultiLinestringFeature> {
		ensure!(
			!self.linestrings.is_empty(),
			"MultiLinestrings must have at least one entry"
		);
		for linestring in &self.linestrings {
			ensure!(
				linestring.len() >= 2,
				"Each entry in MultiLinestrings must have at least two points"
			);
		}

		Ok(MultiLinestringFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry: self.linestrings,
		})
	}

	/// Converts the `VectorTileFeature` into a `MultiPolygonFeature`.
	pub fn to_multi_polygon_feature(self, attributes: &AttributeLookup) -> Result<MultiPolygonFeature> {
		ensure!(!self.linestrings.is_empty(), "Polygons must have at least one entry");
		let mut current_polygon: PolygonGeometry = Vec::new();
		let mut polygons: MultiPolygonGeometry = Vec::new();

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
					polygons.push(current_polygon);
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
			polygons.push(current_polygon);
		}

		Ok(MultiPolygonFeature {
			id: self.id,
			attributes: attributes.translate_tag_ids(&self.tag_ids)?,
			geometry: polygons,
		})
	}
}

/// Decodes linestring geometry from a `BlobReader`.
fn decode_linestring_geometry(reader: &mut BlobReader<LE>) -> Result<MultiLinestringGeometry> {
	// https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding

	let mut linestrings: MultiLinestringGeometry = Vec::new();
	let mut current_linestring: LinestringGeometry = Vec::new();
	let mut x = 0;
	let mut y = 0;

	while reader.has_remaining() {
		let value = reader.read_varint()?;
		let command = value & 0x7;
		let count = value >> 3;

		match command {
			1 | 2 => {
				// MoveTo or LineTo command
				if command == 1 && !current_linestring.is_empty() {
					// MoveTo command indicates the start of a new linestring
					linestrings.push(current_linestring);
					current_linestring = Vec::new();
				}

				for _ in 0..count {
					x += reader.read_svarint()?;
					y += reader.read_svarint()?;
					current_linestring.push(PointGeometry {
						x: x as f64,
						y: y as f64,
					});
				}
			}
			7 => {
				// ClosePath command
				ensure!(
					!current_linestring.is_empty(),
					"ClosePath command found on an empty linestring"
				);
				current_linestring.push(current_linestring[0].clone());
			}
			_ => bail!("Unknown command {}", command),
		}
	}

	if !current_linestring.is_empty() {
		linestrings.push(current_linestring);
	}

	Ok(linestrings)
}
