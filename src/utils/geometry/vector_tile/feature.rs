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
			GeoProperties, LineStringGeometry, MultiFeature, MultiGeometry, MultiLineStringGeometry, MultiPolygonGeometry,
			PointGeometry, PolygonGeometry, Ring,
		},
		BlobReader, BlobWriter,
	},
};
use anyhow::{bail, ensure, Context, Result};
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
			match reader.read_pbf_key().context("Failed to read PBF key")? {
				(1, 0) => f.id = reader.read_varint().context("Failed to read feature ID")?,
				(2, 2) => f.tag_ids = reader.read_pbf_packed_uint32().context("Failed to read tag IDs")?,
				(3, 0) => f.geom_type = GeomType::from(reader.read_varint().context("Failed to read geometry type")?),
				(4, 2) => f.geom_data = reader.read_pbf_blob().context("Failed to read geometry data")?,
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		Ok(f)
	}

	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = BlobWriter::new_le();

		if self.id != 0 {
			writer
				.write_pbf_key(1, 0)
				.context("Failed to write PBF key for feature ID")?;
			writer.write_varint(self.id).context("Failed to write feature ID")?;
		}

		writer
			.write_pbf_key(2, 2)
			.context("Failed to write PBF key for tag IDs")?;
		writer
			.write_pbf_packed_uint32(&self.tag_ids)
			.context("Failed to write tag IDs")?;

		writer
			.write_pbf_key(3, 0)
			.context("Failed to write PBF key for geometry type")?;
		writer
			.write_varint(self.geom_type.as_u64())
			.context("Failed to write geometry type")?;

		writer
			.write_pbf_key(4, 2)
			.context("Failed to write PBF key for geometry data")?;
		writer
			.write_pbf_blob(&self.geom_data)
			.context("Failed to write geometry data")?;

		Ok(writer.into_blob())
	}

	pub fn to_attributes(&self, layer: &VectorTileLayer) -> Result<GeoProperties> {
		layer
			.translate_tag_ids(&self.tag_ids)
			.context("Failed to translate tag IDs to attributes")
	}

	/// Decodes linestring geometry from a `BlobReader`.
	fn decode_geometry(&self) -> Result<MultiLineStringGeometry> {
		// https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding

		let mut reader = BlobReader::new_le(&self.geom_data);

		let mut linestrings: MultiLineStringGeometry = Vec::new();
		let mut current_linestring: LineStringGeometry = Vec::new();
		let mut x = 0;
		let mut y = 0;

		while reader.has_remaining() {
			let value = reader
				.read_varint()
				.context("Failed to read varint for geometry command")?;
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
						x += reader.read_svarint().context("Failed to read x coordinate")?;
						y += reader.read_svarint().context("Failed to read y coordinate")?;
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

		let mut geometry = self.decode_geometry().context("Failed to decode geometry")?;

		match self.geom_type {
			GeomType::Unknown => bail!("Unknown geometry type"),

			GeomType::Point => {
				ensure!(geometry.len() == 1, "(Multi)Points must have exactly one entry");
				let geometry = geometry.pop().unwrap();
				ensure!(!geometry.is_empty(), "The entry in (Multi)Points must not be empty");
				Ok(Point(geometry))
			}

			GeomType::LineString => {
				ensure!(!geometry.is_empty(), "MultiLineStrings must have at least one entry");
				for line in &geometry {
					ensure!(
						line.len() >= 2,
						"Each entry in MultiLineStrings must have at least two points"
					);
				}
				Ok(LineString(geometry))
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
			self.to_geometry().context("Failed to convert to geometry")?,
			self.to_attributes(layer).context("Failed to convert to attributes")?,
		))
	}
}
