#![allow(dead_code)]

use super::{geometry_type::GeomType, layer::VectorTileLayer};
use crate::ext::validate;
use crate::geo::{GeoFeature, GeoProperties, GeoValue};
use anyhow::{Context, Result, bail, ensure};
use byteorder::LE;
use geo_types::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use versatiles_core::{
	Blob,
	io::{ValueReader, ValueReaderSlice, ValueWriter, ValueWriterBlob},
	utils::float_to_int,
};

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

/// Returns 2 × the signed area of a closed ring (trapezoid form, matching the
/// historical versatiles convention: positive for counter-clockwise rings).
fn ring_signed_double_area(coords: &[Coord<f64>]) -> f64 {
	let n = coords.len();
	if n < 3 {
		return 0.0;
	}
	let mut sum = 0.0;
	let mut prev = coords[n - 1];
	for &p in coords {
		sum += (prev.x - p.x) * (p.y + prev.y);
		prev = p;
	}
	sum
}

impl VectorTileFeature {
	/// Decodes a `VectorTileFeature` from a `BlobReader`.
	pub fn read(reader: &mut dyn ValueReader<'_, LE>) -> Result<VectorTileFeature> {
		let mut f = VectorTileFeature::default();

		while reader.has_remaining()? {
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
		let mut writer = ValueWriterBlob::new_le();

		if let Some(id) = self.id {
			writer
				.write_pbf_key(1, 0)
				.context("Failed to write PBF key for feature ID")?;
			writer.write_varint(id).context("Failed to write feature ID")?;
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

	pub fn to_geometry(&self) -> Result<Geometry<f64>> {
		// https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding

		let coordinates = {
			let mut reader = ValueReaderSlice::new_le(self.geom_data.as_slice());

			let mut lines: Vec<Vec<Coord<f64>>> = Vec::new();
			let mut line: Vec<Coord<f64>> = Vec::new();
			let mut x = 0i64;
			let mut y = 0i64;

			while reader.has_remaining()? {
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

							line.push(Coord {
								x: x as f64,
								y: y as f64,
							});
						}
					}
					7 => {
						// ClosePath command
						ensure!(!line.is_empty(), "ClosePath command found on an empty linestring");
						line.push(line[0]);
					}
					_ => bail!("Unknown command {command}"),
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
				ensure!(!coordinates.is_empty(), "(Multi)Points must not be empty");

				let points = coordinates
					.into_iter()
					.map(|mut entry| {
						ensure!(entry.len() == 1, "(Multi)Point entries must have exactly one entry");
						Ok(Point(entry.pop().expect("ensured len == 1 above")))
					})
					.collect::<Result<Vec<Point<f64>>>>()?;
				Ok(Geometry::MultiPoint(MultiPoint(points)))
			}

			GeomType::MultiLineString => {
				ensure!(!coordinates.is_empty(), "MultiLineStrings must have at least one entry");
				let lines = coordinates.into_iter().map(LineString::new).collect::<Vec<_>>();
				let g = Geometry::MultiLineString(MultiLineString(lines));
				validate(&g).context("Invalid MultiLineString")?;
				Ok(g)
			}

			GeomType::MultiPolygon => {
				ensure!(!coordinates.is_empty(), "Polygons must have at least one entry");
				let mut current_polygon: Vec<LineString<f64>> = Vec::new();
				let mut polygons: Vec<Polygon<f64>> = Vec::new();

				let push_polygon = |rings: Vec<LineString<f64>>, polygons: &mut Vec<Polygon<f64>>| {
					if let Some((exterior, interiors)) = rings.split_first() {
						polygons.push(Polygon::new(exterior.clone(), interiors.to_vec()));
					}
				};

				for ring_coords in coordinates {
					let area2 = ring_signed_double_area(&ring_coords);
					ensure!(ring_coords.len() >= 4, "polygon ring must have at least 4 points");
					ensure!(ring_coords.first() == ring_coords.last(), "polygon ring must be closed");
					let ring = LineString::new(ring_coords);

					if area2 > 1e-14 {
						// Outer ring
						if !current_polygon.is_empty() {
							push_polygon(std::mem::take(&mut current_polygon), &mut polygons);
						}
						current_polygon.push(ring);
					} else if area2 < -1e-14 {
						// Inner ring
						if current_polygon.is_empty() {
							log::trace!("An outer ring must precede inner rings");
						} else {
							current_polygon.push(ring);
						}
					} else {
						log::trace!("Error: Ring with zero area");
					}
				}

				if !current_polygon.is_empty() {
					push_polygon(current_polygon, &mut polygons);
				}

				Ok(Geometry::MultiPolygon(MultiPolygon(polygons)))
			}
		}
	}

	pub fn decode_properties(&self, layer: &VectorTileLayer) -> Result<GeoProperties> {
		layer.decode_tag_ids(&self.tag_ids)
	}

	pub fn to_feature(&self, layer: &VectorTileLayer) -> Result<GeoFeature> {
		let mut feature = GeoFeature::new(self.to_geometry().context("Failed to convert to geometry")?);

		if let Some(id) = self.id {
			feature.set_id(GeoValue::from(id));
		}

		feature.properties = layer.decode_tag_ids(&self.tag_ids)?;

		Ok(feature)
	}

	pub fn from_geometry(id: Option<u64>, tag_ids: Vec<u32>, geometry: Geometry<f64>) -> Result<VectorTileFeature> {
		fn write_coord(writer: &mut ValueWriterBlob<LE>, coord0: &mut (i64, i64), coord: Coord<f64>) -> Result<()> {
			let x = float_to_int(coord.x)?;
			let y = float_to_int(coord.y)?;
			writer.write_svarint(x - coord0.0)?;
			writer.write_svarint(y - coord0.1)?;
			coord0.0 = x;
			coord0.1 = y;
			Ok(())
		}

		fn write_points(points: MultiPoint<f64>) -> Result<Blob> {
			let mut writer = ValueWriterBlob::new_le();
			let point0 = &mut (0i64, 0i64);
			writer.write_varint(((points.0.len() as u64) << 3) | 0x1)?;
			for point in points.0 {
				write_coord(&mut writer, point0, point.0)?;
			}
			Ok(writer.into_blob())
		}

		fn write_line_strings(line_strings: MultiLineString<f64>) -> Result<Blob> {
			let mut writer = ValueWriterBlob::new_le();
			let point0 = &mut (0i64, 0i64);

			for line_string in line_strings.0 {
				let coords = line_string.0;
				let Some((first, rest)) = coords.split_first() else {
					continue;
				};

				// Write the MoveTo command for the first point
				writer.write_varint((1 << 3) | 0x1)?; // MoveTo command
				write_coord(&mut writer, point0, *first)?;

				// Write the LineTo command for the remaining points
				if !rest.is_empty() {
					writer.write_varint(((rest.len() as u64) << 3) | 0x2)?; // LineTo command
					for &point in rest {
						write_coord(&mut writer, point0, point)?;
					}
				}
			}

			Ok(writer.into_blob())
		}

		fn write_ring(writer: &mut ValueWriterBlob<LE>, point0: &mut (i64, i64), ring: &LineString<f64>) -> Result<()> {
			let coords = &ring.0;
			// Drop closing duplicate vertex if present (MVT closes via ClosePath, not by repeating).
			let trim_to = if coords.len() >= 2 && coords.first() == coords.last() {
				coords.len() - 1
			} else {
				coords.len()
			};
			if trim_to < 3 {
				// degenerate ring; skip
				return Ok(());
			}
			let coords = &coords[..trim_to];

			let (first, rest) = coords.split_first().expect("trim_to >= 3");

			writer.write_varint((1 << 3) | 0x1)?; // MoveTo command
			write_coord(writer, point0, *first)?;

			if !rest.is_empty() {
				writer.write_varint(((rest.len() as u64) << 3) | 0x2)?; // LineTo command
				for &point in rest {
					write_coord(writer, point0, point)?;
				}
			}

			writer.write_varint(7)?; // ClosePath command
			Ok(())
		}

		fn write_polygons(polygons: MultiPolygon<f64>) -> Result<Blob> {
			let mut writer = ValueWriterBlob::new_le();
			let point0 = &mut (0i64, 0i64);

			for polygon in polygons.0 {
				let (exterior, interiors) = polygon.into_inner();
				write_ring(&mut writer, point0, &exterior)?;
				for interior in interiors {
					write_ring(&mut writer, point0, &interior)?;
				}
			}

			Ok(writer.into_blob())
		}

		let (geom_type, geom_data) = match geometry {
			Geometry::Point(p) => (GeomType::MultiPoint, write_points(MultiPoint(vec![p]))?),
			Geometry::MultiPoint(mp) => (GeomType::MultiPoint, write_points(mp)?),
			Geometry::LineString(ls) => (
				GeomType::MultiLineString,
				write_line_strings(MultiLineString(vec![ls]))?,
			),
			Geometry::MultiLineString(ml) => (GeomType::MultiLineString, write_line_strings(ml)?),
			Geometry::Polygon(p) => (GeomType::MultiPolygon, write_polygons(MultiPolygon(vec![p]))?),
			Geometry::MultiPolygon(mp) => (GeomType::MultiPolygon, write_polygons(mp)?),
			Geometry::Line(_) => bail!("MVT encoding of Line is not supported"),
			Geometry::Rect(_) => bail!("MVT encoding of Rect is not supported"),
			Geometry::Triangle(_) => bail!("MVT encoding of Triangle is not supported"),
			Geometry::GeometryCollection(_) => bail!("MVT encoding of GeometryCollection is not supported"),
		};

		Ok(VectorTileFeature {
			id,
			tag_ids,
			geom_type,
			geom_data,
		})
	}

	#[cfg(test)]
	pub fn new_example() -> Self {
		VectorTileFeature::from_geometry(Some(3), vec![1, 2], crate::geo::example_geometry()).unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

	fn ls_from(pts: &[[i32; 2]]) -> LineString<f64> {
		LineString::from(
			pts.iter()
				.map(|p| [f64::from(p[0]), f64::from(p[1])])
				.collect::<Vec<_>>(),
		)
	}

	fn polygon_from(rings: &[Vec<[i32; 2]>]) -> Polygon<f64> {
		let mut iter = rings.iter().map(|ring| ls_from(ring));
		let exterior = iter.next().expect("polygon has exterior");
		let interiors = iter.collect();
		Polygon::new(exterior, interiors)
	}

	fn round_trip_feature(geometry: Geometry<f64>) -> Result<()> {
		let vector_tile_feature = VectorTileFeature::from_geometry(None, vec![], geometry.clone())?;
		let decoded = vector_tile_feature.to_geometry()?;
		assert_eq!(canonical_multi(geometry), decoded);
		Ok(())
	}

	/// Lifts single-geometry variants into their multi equivalents so that
	/// MVT round-trips (which always decode into multi geometries) compare cleanly.
	fn canonical_multi(g: Geometry<f64>) -> Geometry<f64> {
		match g {
			Geometry::Point(p) => Geometry::MultiPoint(MultiPoint(vec![p])),
			Geometry::LineString(ls) => Geometry::MultiLineString(MultiLineString(vec![ls])),
			Geometry::Polygon(p) => Geometry::MultiPolygon(MultiPolygon(vec![p])),
			other => other,
		}
	}

	#[test]
	fn point_geometry_round_trip() -> Result<()> {
		round_trip_feature(Geometry::Point(Point::new(1.0, 2.0)))
	}

	#[test]
	fn line_string_geometry_round_trip() -> Result<()> {
		round_trip_feature(Geometry::LineString(ls_from(&[[0, 1], [0, 3]])))
	}

	#[test]
	fn polygon_geometry_round_trip() -> Result<()> {
		let p = polygon_from(&[
			vec![[0, 0], [3, 0], [3, 3], [0, 3], [0, 0]],
			vec![[1, 1], [1, 2], [2, 2], [1, 1]],
		]);
		round_trip_feature(Geometry::Polygon(p))
	}

	#[test]
	fn multi_point_geometry_round_trip() -> Result<()> {
		let mp = MultiPoint(vec![Point::new(2.0, 3.0), Point::new(4.0, 5.0)]);
		round_trip_feature(Geometry::MultiPoint(mp))
	}

	#[test]
	fn multi_line_string_geometry_round_trip() -> Result<()> {
		let ml = MultiLineString(vec![
			ls_from(&[[0, 0], [1, 1], [2, 0]]),
			ls_from(&[[0, 2], [1, 1], [2, 2]]),
		]);
		round_trip_feature(Geometry::MultiLineString(ml))
	}

	#[test]
	fn multi_polygon_geometry_round_trip() -> Result<()> {
		let mp = MultiPolygon(vec![
			polygon_from(&[
				vec![[0, 0], [3, 0], [3, 3], [0, 3], [0, 0]],
				vec![[1, 1], [1, 2], [2, 2], [1, 1]],
			]),
			polygon_from(&[
				vec![[4, 0], [7, 0], [7, 3], [4, 3], [4, 0]],
				vec![[5, 1], [5, 2], [6, 2], [5, 1]],
			]),
		]);
		round_trip_feature(Geometry::MultiPolygon(mp))
	}

	#[test]
	fn rejects_unsupported_variants() {
		let l = geo_types::Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 });
		assert!(VectorTileFeature::from_geometry(None, vec![], Geometry::Line(l)).is_err());
	}

	fn coords(pts: &[(f64, f64)]) -> Vec<Coord<f64>> {
		pts.iter().map(|&(x, y)| Coord { x, y }).collect()
	}

	#[test]
	fn ring_signed_double_area_ccw_is_positive() {
		// Unit square wound counter-clockwise: 2 × area = 2.
		let ring = coords(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]);
		assert!((ring_signed_double_area(&ring) - 2.0).abs() < 1e-12);
	}

	#[test]
	fn ring_signed_double_area_cw_is_negative() {
		// Same square wound clockwise: 2 × area = -2.
		let ring = coords(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]);
		assert!((ring_signed_double_area(&ring) - (-2.0)).abs() < 1e-12);
	}

	#[test]
	fn ring_signed_double_area_degenerate_returns_zero() {
		// Fewer than 3 points: not a ring; defined to be 0.
		assert!(ring_signed_double_area(&[]).abs() < 1e-12);
		assert!(ring_signed_double_area(&coords(&[(0.0, 0.0)])).abs() < 1e-12);
		assert!(ring_signed_double_area(&coords(&[(0.0, 0.0), (1.0, 1.0)])).abs() < 1e-12);
	}

	#[test]
	fn ring_signed_double_area_collinear_is_zero() {
		// Three collinear points enclose no area.
		let ring = coords(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (0.0, 0.0)]);
		assert!(ring_signed_double_area(&ring).abs() < 1e-12);
	}
}
