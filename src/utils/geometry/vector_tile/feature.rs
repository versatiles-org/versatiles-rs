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
			Feature, GeoProperties, Geometry, LineStringGeometry, MultiPolygonGeometry, PointGeometry, PolygonGeometry,
			Ring,
		},
		BlobReader, BlobWriter,
	},
};
use anyhow::{bail, ensure, Context, Result};
use byteorder::LE;

#[derive(Debug, PartialEq)]
pub struct VectorTileFeature {
	pub id: Option<u64>,
	pub tag_ids: Vec<u32>,
	pub geom_type: GeomType,
	pub geom_data: Blob,
}

impl Default for VectorTileFeature {
	fn default() -> Self {
		VectorTileFeature {
			id: None,
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
				(1, 0) => f.id = Some(reader.read_varint().context("Failed to read feature ID")?),
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

		if let Some(id) = self.id {
			writer
				.write_pbf_key(1, 0)
				.context("Failed to write PBF key for feature ID")?;
			writer.write_varint(id).context("Failed to write feature ID")?;
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

	pub fn to_properties(&self, layer: &VectorTileLayer) -> Result<GeoProperties> {
		layer
			.translate_tag_ids(&self.tag_ids)
			.context("Failed to translate tag IDs to attributes")
	}

	pub fn to_geometry(&self) -> Result<Geometry> {
		// https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding

		let geometry = {
			let mut reader = BlobReader::new_le(&self.geom_data);

			let mut lines: Vec<Vec<PointGeometry>> = Vec::new();
			let mut line: Vec<PointGeometry> = Vec::new();
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
						for _ in 0..count {
							if command == 1 && !line.is_empty() {
								// MoveTo command indicates the start of a new linestring
								lines.push(line);
								line = Vec::new();
							}

							x += reader.read_svarint().context("Failed to read x coordinate")?;
							y += reader.read_svarint().context("Failed to read y coordinate")?;

							line.push(PointGeometry {
								x: x as f64,
								y: y as f64,
							});
						}
					}
					7 => {
						// ClosePath command
						ensure!(!line.is_empty(), "ClosePath command found on an empty linestring");
						line.push(line[0].clone());
					}
					_ => bail!("Unknown command {}", command),
				}
			}

			if !line.is_empty() {
				lines.push(line);
			}

			lines
		};

		match self.geom_type {
			GeomType::Unknown => bail!("Unknown geometry type"),

			GeomType::MultiPoint => {
				ensure!(!geometry.is_empty(), "(Multi)Points must not be empty");

				Ok(Geometry::new_multi_point(
					geometry
						.into_iter()
						.map(|mut line| {
							ensure!(line.len() == 1, "(Multi)Point entries must have exactly one entry");
							Ok(line.pop().unwrap())
						})
						.collect::<Result<Vec<PointGeometry>>>()?,
				))
			}

			GeomType::MultiLineString => {
				ensure!(!geometry.is_empty(), "MultiLineStrings must have at least one entry");
				for line in &geometry {
					ensure!(
						line.len() >= 2,
						"Each entry in MultiLineStrings must have at least two points"
					);
				}
				Ok(Geometry::new_multi_line_string(geometry))
			}

			GeomType::MultiPolygon => {
				ensure!(!geometry.is_empty(), "Polygons must have at least one entry");
				let mut current_polygon = PolygonGeometry::new();
				let mut polygons = MultiPolygonGeometry::new();

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

				Ok(Geometry::new_multi_polygon(polygons))
			}
		}
	}

	pub fn to_feature(&self, layer: &VectorTileLayer) -> Result<Feature> {
		let mut feature = Feature::new(self.to_geometry().context("Failed to convert to geometry")?);

		if let Some(id) = self.id {
			feature.set_id(id);
		}

		feature.set_properties(self.to_properties(layer).context("Failed to convert to attributes")?);

		Ok(feature)
	}

	pub fn from_geometry(id: Option<u64>, tag_ids: Vec<u32>, geometry: Geometry) -> Result<VectorTileFeature> {
		fn write_point(writer: &mut BlobWriter<LE>, point0: &mut (i64, i64), point: &PointGeometry) -> Result<()> {
			let x = point.x.round() as i64;
			let y = point.y.round() as i64;
			writer.write_svarint(x - point0.0)?;
			writer.write_svarint(y - point0.1)?;
			point0.0 = x;
			point0.1 = y;
			Ok(())
		}

		fn write_points(points: Vec<PointGeometry>) -> Result<Blob> {
			let mut writer = BlobWriter::new_le();
			let point0 = &mut (0i64, 0i64);
			writer.write_varint((points.len() as u64) << 3 | 0x1)?;
			for point in &points {
				write_point(&mut writer, point0, point)?
			}
			Ok(writer.into_blob())
		}

		fn write_line_strings(line_strings: Vec<LineStringGeometry>) -> Result<Blob> {
			let mut writer = BlobWriter::new_le();
			let point0 = &mut (0i64, 0i64);

			for line_string in line_strings {
				if line_string.is_empty() {
					continue;
				}

				// Write the MoveTo command for the first point
				writer.write_varint(1 << 3 | 0x1)?; // MoveTo command
				write_point(&mut writer, point0, &line_string[0])?;

				// Write the LineTo command for the remaining points
				if line_string.len() > 1 {
					writer.write_varint((line_string.len() as u64 - 1) << 3 | 0x2)?; // LineTo command
					for point in &line_string[1..] {
						write_point(&mut writer, point0, point)?;
					}
				}
			}

			Ok(writer.into_blob())
		}

		fn write_polygons(polygons: Vec<PolygonGeometry>) -> Result<Blob> {
			let mut writer = BlobWriter::new_le();
			let point0 = &mut (0i64, 0i64);

			for polygon in polygons {
				for ring in polygon {
					if ring.is_empty() {
						continue;
					}

					// Write the MoveTo command for the first point
					writer.write_varint(1 << 3 | 0x1)?; // MoveTo command
					write_point(&mut writer, point0, &ring[0])?;

					// Write the LineTo command for the remaining points
					if ring.len() > 2 {
						writer.write_varint((ring.len() as u64 - 2) << 3 | 0x1)?; // LineTo command
						for point in &ring[1..ring.len() - 1] {
							write_point(&mut writer, point0, point)?;
						}
					}

					// Write the ClosePath command
					writer.write_varint(7)?; // ClosePath command
				}
			}

			Ok(writer.into_blob())
		}

		use crate::utils::geometry::types::GeometryValue::*;
		let (geom_type, geom_data) = match geometry.value {
			Point(g) => (GeomType::MultiPoint, write_points(vec![g])?),
			MultiPoint(g) => (GeomType::MultiPoint, write_points(g)?),
			LineString(g) => (GeomType::MultiLineString, write_line_strings(vec![g])?),
			MultiLineString(g) => (GeomType::MultiLineString, write_line_strings(g)?),
			Polygon(g) => (GeomType::MultiPolygon, write_polygons(vec![g])?),
			MultiPolygon(g) => (GeomType::MultiPolygon, write_polygons(g)?),
		};

		Ok(VectorTileFeature {
			tag_ids,
			id,
			geom_type,
			geom_data,
		})
	}
}
