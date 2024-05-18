#![allow(dead_code)]

use super::{
	geometry_type::GeomType,
	layer::VectorTileLayer,
	utils::{BlobReaderPBF, BlobWriterPBF},
};
use crate::{
	types::Blob,
	utils::{
		geometry::types::{
			GeoProperties, LinestringGeometry, MultiFeature, MultiGeometry, MultiLinestringGeometry, MultiPolygonGeometry,
			PointGeometry, PolygonGeometry, Ring,
		},
		BlobReader, BlobWriter,
	},
};
use anyhow::{bail, ensure, Result};
use byteorder::LE;

#[derive(Debug, PartialEq)]
pub struct VectorTileFeature {
	pub id: u64,
	pub tag_ids: Vec<u32>,
	pub geom_type: GeomType,
	pub geom_data: Blob,
}

impl Default for VectorTileFeature {
	fn default() -> Self {
		VectorTileFeature {
			id: 0,
			tag_ids: Vec::new(),
			geom_type: GeomType::Unknown,
			geom_data: Blob::new_empty(),
		}
	}
}

impl VectorTileFeature {
	/// Decodes a `VectorTileFeature` from a `BlobReader`.
	pub fn read(reader: &mut BlobReader<LE>) -> Result<VectorTileFeature> {
		let mut f = VectorTileFeature::default();

		while reader.has_remaining() {
			match reader.read_pbf_key()? {
				(1, 0) => f.id = reader.read_varint()?,
				(2, 2) => f.tag_ids = reader.read_pbf_packed_uint32()?,
				(3, 0) => f.geom_type = GeomType::from(reader.read_varint()?),
				(4, 2) => f.geom_data = reader.read_pbf_blob()?,
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		Ok(f)
	}

	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = BlobWriter::new_le();

		if self.id != 0 {
			writer.write_pbf_key(1, 0)?;
			writer.write_varint(self.id)?;
		}

		writer.write_pbf_key(2, 2)?;
		writer.write_pbf_packed_uint32(&self.tag_ids)?;

		writer.write_pbf_key(3, 0)?;
		writer.write_varint(self.geom_type.as_u64())?;

		writer.write_pbf_key(4, 2)?;
		writer.write_pbf_blob(&self.geom_data)?;

		Ok(writer.into_blob())
	}

	pub fn to_attributes(&self, layer: &VectorTileLayer) -> Result<GeoProperties> {
		layer.translate_tag_ids(&self.tag_ids)
	}

	/// Decodes linestring geometry from a `BlobReader`.
	fn decode_geometry(&self) -> Result<MultiLinestringGeometry> {
		// https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding

		let mut reader = BlobReader::new_le(&self.geom_data);

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

	pub fn to_geometry(&self) -> Result<MultiGeometry> {
		use MultiGeometry::*;

		let mut geometry = self.decode_geometry()?;

		match self.geom_type {
			GeomType::Unknown => bail!("unknown geometry"),

			GeomType::Point => {
				ensure!(geometry.len() == 1, "(Multi)Points must have exactly one entry");
				let geometry = geometry.pop().unwrap();
				ensure!(!geometry.is_empty(), "The entry in (Multi)Points must not be empty");
				Ok(Point(geometry))
			}

			GeomType::Linestring => {
				ensure!(!geometry.is_empty(), "MultiLinestrings must have at least one entry");
				for line in &geometry {
					ensure!(
						line.len() >= 2,
						"Each entry in MultiLinestrings must have at least two points"
					);
				}
				Ok(Linestring(geometry))
			}

			GeomType::Polygon => {
				ensure!(!geometry.is_empty(), "Polygons must have at least one entry");
				let mut current_polygon: PolygonGeometry = Vec::new();
				let mut polygons: MultiPolygonGeometry = Vec::new();

				for ring in geometry {
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

				Ok(Polygon(polygons))
			}
		}
	}

	pub fn to_feature(&self, layer: &VectorTileLayer) -> Result<MultiFeature> {
		Ok(MultiFeature::new(
			Some(self.id),
			self.to_geometry()?,
			self.to_attributes(layer)?,
		))
	}
}
