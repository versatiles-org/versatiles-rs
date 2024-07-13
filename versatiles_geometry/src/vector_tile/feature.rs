#![allow(dead_code)]

use super::{geometry_type::GeomType, layer::VectorTileLayer};
use crate::{
	geo::*,
	math::area_ring,
	types::Blob,
	utils::io::{ValueReader, ValueReaderSlice, ValueWriter, ValueWriterBlob},
};
use anyhow::{bail, ensure, Context, Result};
use byteorder::LE;
use log::trace;

#[derive(Clone, Debug, PartialEq)]
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
	pub fn read(reader: &mut dyn ValueReader<'_, LE>) -> Result<VectorTileFeature> {
		let mut f = VectorTileFeature::default();

		while reader.has_remaining() {
			match reader.read_pbf_key().context("Failed to read PBF key")? {
				(1, 0) => f.id = Some(reader.read_varint().context("Failed to read feature ID")?),
				(2, 2) => {
					f.tag_ids = reader
						.read_pbf_packed_uint32()
						.context("Failed to read tag IDs")?
				}
				(3, 0) => {
					f.geom_type = GeomType::from(
						reader
							.read_varint()
							.context("Failed to read geometry type")?,
					)
				}
				(4, 2) => {
					f.geom_data = reader
						.read_pbf_blob()
						.context("Failed to read geometry data")?
				}
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		Ok(f)
	}

	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_le();

		if let Some(id) = self.id {
			writer
				.write_pbf_key(1, 0)
				.context("Failed to write PBF key for feature ID")?;
			writer
				.write_varint(id)
				.context("Failed to write feature ID")?;
		}

		if !self.tag_ids.is_empty() {
			writer
				.write_pbf_key(2, 2)
				.context("Failed to write PBF key for tag IDs")?;
			writer
				.write_pbf_packed_uint32(&self.tag_ids)
				.context("Failed to write tag IDs")?;
		}

		writer
			.write_pbf_key(3, 0)
			.context("Failed to write PBF key for geometry type")?;
		writer
			.write_varint(self.geom_type.as_u64())
			.context("Failed to write geometry type")?;

		if !self.geom_data.is_empty() {
			writer
				.write_pbf_key(4, 2)
				.context("Failed to write PBF key for geometry data")?;
			writer
				.write_pbf_blob(&self.geom_data)
				.context("Failed to write geometry data")?;
		}

		Ok(writer.into_blob())
	}

	pub fn to_geometry(&self) -> Result<Geometry> {
		// https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding

		let geometry = {
			let mut reader = ValueReaderSlice::new_le(self.geom_data.as_slice());

			let mut lines: Coordinates2 = Vec::new();
			let mut line: Coordinates1 = Vec::new();
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

							x += reader
								.read_svarint()
								.context("Failed to read x coordinate")?;
							y += reader
								.read_svarint()
								.context("Failed to read y coordinate")?;

							line.push([x as f64, y as f64]);
						}
					}
					7 => {
						// ClosePath command
						ensure!(
							!line.is_empty(),
							"ClosePath command found on an empty linestring"
						);
						line.push(line[0]);
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
							ensure!(
								line.len() == 1,
								"(Multi)Point entries must have exactly one entry"
							);
							Ok(line.pop().unwrap())
						})
						.collect::<Result<Coordinates1>>()?,
				))
			}

			GeomType::MultiLineString => {
				ensure!(
					!geometry.is_empty(),
					"MultiLineStrings must have at least one entry"
				);
				for line in &geometry {
					ensure!(
						line.len() >= 2,
						"Each entry in MultiLineStrings must have at least two points"
					);
				}
				Ok(Geometry::new_multi_line_string(geometry))
			}

			GeomType::MultiPolygon => {
				ensure!(
					!geometry.is_empty(),
					"Polygons must have at least one entry"
				);
				let mut current_polygon = Vec::new();
				let mut polygons = Vec::new();

				for ring in geometry {
					ensure!(
						ring.len() >= 4,
						"Each ring in Polygons must have at least four points (A,B,C,A)"
					);

					ensure!(
						ring[0] == ring[ring.len() - 1],
						"First and last point of the ring must be the same"
					);

					let area = area_ring(&ring);

					if area > 1e-14 {
						// Outer ring
						if !current_polygon.is_empty() {
							polygons.push(current_polygon);
							current_polygon = Vec::new();
						}
						current_polygon.push(ring);
					} else if area < -1e-14 {
						// Inner ring
						if current_polygon.is_empty() {
							trace!("An outer ring must precede inner rings");
						} else {
							current_polygon.push(ring);
						}
					} else {
						trace!("Error: Ring with zero area")
					}
				}

				if !current_polygon.is_empty() {
					polygons.push(current_polygon);
				}

				Ok(Geometry::new_multi_polygon(polygons))
			}
		}
	}

	pub fn decode_properties(&self, layer: &VectorTileLayer) -> Result<GeoProperties> {
		layer.decode_tag_ids(&self.tag_ids)
	}

	pub fn to_feature(&self, layer: &VectorTileLayer) -> Result<Feature> {
		let mut feature = Feature::new(
			self
				.to_geometry()
				.context("Failed to convert to geometry")?,
		);

		if let Some(id) = self.id {
			feature.set_id(id);
		}

		feature.properties = layer.decode_tag_ids(&self.tag_ids)?;

		Ok(feature)
	}

	pub fn from_geometry(
		id: Option<u64>,
		tag_ids: Vec<u32>,
		geometry: Geometry,
	) -> Result<VectorTileFeature> {
		fn write_point(
			writer: &mut ValueWriterBlob<LE>,
			point0: &mut (i64, i64),
			point: PointGeometry,
		) -> Result<()> {
			let x = point.0[0].round() as i64;
			let y = point.0[1].round() as i64;
			writer.write_svarint(x - point0.0)?;
			writer.write_svarint(y - point0.1)?;
			point0.0 = x;
			point0.1 = y;
			Ok(())
		}

		fn write_points(points: MultiPointGeometry) -> Result<Blob> {
			let mut writer = ValueWriterBlob::new_le();
			let point0 = &mut (0i64, 0i64);
			writer.write_varint((points.len() as u64) << 3 | 0x1)?;
			for point in points.into_iter() {
				write_point(&mut writer, point0, point)?
			}
			Ok(writer.into_blob())
		}

		fn write_line_strings(line_strings: MultiLineStringGeometry) -> Result<Blob> {
			let mut writer = ValueWriterBlob::new_le();
			let point0 = &mut (0i64, 0i64);

			for line_string in line_strings.into_iter() {
				if line_string.is_empty() {
					continue;
				}

				let (first, rest) = line_string.into_first_and_rest();

				// Write the MoveTo command for the first point
				writer.write_varint(1 << 3 | 0x1)?; // MoveTo command
				write_point(&mut writer, point0, first)?;

				// Write the LineTo command for the remaining points
				if !rest.is_empty() {
					writer.write_varint((rest.len() as u64) << 3 | 0x2)?; // LineTo command
					for point in rest.into_iter() {
						write_point(&mut writer, point0, point)?;
					}
				}
			}

			Ok(writer.into_blob())
		}

		fn write_polygons(polygons: MultiPolygonGeometry) -> Result<Blob> {
			let mut writer = ValueWriterBlob::new_le();
			let point0 = &mut (0i64, 0i64);

			for polygon in polygons.into_iter() {
				for ring in polygon.into_iter() {
					if ring.len() < 4 {
						continue;
					}

					let (first, mut rest) = ring.into_first_and_rest();
					rest.pop();

					// Write the MoveTo command for the first point
					writer.write_varint(1 << 3 | 0x1)?; // MoveTo command
					write_point(&mut writer, point0, first)?;

					// Write the LineTo command for the remaining points
					if !rest.is_empty() {
						writer.write_varint((rest.len() as u64) << 3 | 0x2)?; // LineTo command
						for point in rest.into_iter() {
							write_point(&mut writer, point0, point)?;
						}
					}

					// Write the ClosePath command
					writer.write_varint(7)?; // ClosePath command
				}
			}

			Ok(writer.into_blob())
		}

		fn m<T>(g: &[T]) -> Vec<&T> {
			g.iter().collect()
		}
		use crate::geo::Geometry::*;
		let (geom_type, geom_data) = match geometry {
			Point(g) => (GeomType::MultiPoint, write_points(g.into_multi())?),
			MultiPoint(g) => (GeomType::MultiPoint, write_points(g)?),
			LineString(g) => (
				GeomType::MultiLineString,
				write_line_strings(g.into_multi())?,
			),
			MultiLineString(g) => (GeomType::MultiLineString, write_line_strings(g)?),
			Polygon(g) => (GeomType::MultiPolygon, write_polygons(g.into_multi())?),
			MultiPolygon(g) => (GeomType::MultiPolygon, write_polygons(g)?),
		};

		Ok(VectorTileFeature {
			tag_ids,
			id,
			geom_type,
			geom_data,
		})
	}

	#[cfg(test)]
	pub fn new_example() -> Self {
		VectorTileFeature::from_geometry(Some(3), vec![1, 2], Geometry::new_example()).unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn round_trip_feature(geometry: Geometry) -> Result<()> {
		// Convert to VectorTileFeature
		let vector_tile_feature = VectorTileFeature::from_geometry(None, vec![], geometry.clone())?;

		// Compare original and converted features
		assert_eq!(geometry.into_multi(), vector_tile_feature.to_geometry()?);
		Ok(())
	}

	#[test]
	fn point_geometry_round_trip() -> Result<()> {
		let geometry = Geometry::new_point([1, 2]);
		round_trip_feature(geometry)
	}

	#[test]
	fn line_string_geometry_round_trip() -> Result<()> {
		let geometry = Geometry::new_line_string(vec![[0, 1], [0, 3]]);
		round_trip_feature(geometry)
	}

	#[test]
	fn polygon_geometry_round_trip() -> Result<()> {
		let geometry = Geometry::new_polygon(vec![
			vec![[0, 0], [3, 0], [3, 3], [0, 3], [0, 0]],
			vec![[1, 1], [1, 2], [2, 2], [1, 1]],
		]);
		round_trip_feature(geometry)
	}

	#[test]
	fn multi_point_geometry_round_trip() -> Result<()> {
		let geometry = Geometry::new_multi_point(vec![[2, 3], [4, 5]]);
		round_trip_feature(geometry)
	}

	#[test]
	fn multi_line_string_geometry_round_trip() -> Result<()> {
		let geometry = Geometry::new_multi_line_string(vec![
			vec![[0, 0], [1, 1], [2, 0]],
			vec![[0, 2], [1, 1], [2, 2]],
		]);
		round_trip_feature(geometry)
	}

	#[test]
	fn multi_polygon_geometry_round_trip() -> Result<()> {
		let geometry = Geometry::new_multi_polygon(vec![
			vec![
				vec![[0, 0], [3, 0], [3, 3], [0, 3], [0, 0]],
				vec![[1, 1], [1, 2], [2, 2], [1, 1]],
			],
			vec![
				vec![[4, 0], [7, 0], [7, 3], [4, 3], [4, 0]],
				vec![[5, 1], [5, 2], [6, 2], [5, 1]],
			],
		]);
		round_trip_feature(geometry)
	}
}
