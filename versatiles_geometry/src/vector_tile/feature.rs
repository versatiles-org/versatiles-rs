#![allow(dead_code)]

use super::{geometry_type::GeomType, layer::VectorTileLayer};
use crate::ext::validate;
use crate::geo::{GeoFeature, GeoProperties, GeoValue};
use anyhow::{Context, Result, bail, ensure};
use byteorder::LE;
use geo_types::{
	Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
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
pub(super) fn ring_signed_double_area(coords: &[Coord<f64>]) -> f64 {
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

/// Threshold below which a ring's signed double area is treated as zero
/// (degenerate ring — collinear vertices or floating-point noise). Mirrors
/// the threshold used by [`VectorTileFeature::to_geometry`].
pub(super) const WINDING_EPSILON: f64 = 1e-14;

/// Rewinds the rings of a polygon so they conform to MVT 2.1 §4.3.3.3:
/// the exterior ring has positive surveyor area, each interior ring has
/// negative area. Rings whose area is within `WINDING_EPSILON` of zero
/// (degenerate) are left as-is; the encoder is responsible for filtering them.
///
/// Called by `VectorTileFeature::from_geometry` (via `write_polygons`) so that
/// any `Polygon`/`MultiPolygon` handed to the encoder ends up spec-conformant
/// on disk regardless of where the input geometry came from.
pub(crate) fn normalize_polygon_winding(poly: Polygon<f64>) -> Polygon<f64> {
	let (mut exterior, interiors) = poly.into_inner();
	if ring_signed_double_area(&exterior.0) < -WINDING_EPSILON {
		exterior.0.reverse();
	}
	let interiors = interiors
		.into_iter()
		.map(|mut interior| {
			if ring_signed_double_area(&interior.0) > WINDING_EPSILON {
				interior.0.reverse();
			}
			interior
		})
		.collect();
	Polygon::new(exterior, interiors)
}

/// `normalize_polygon_winding` lifted over each polygon in a `MultiPolygon`.
pub(crate) fn normalize_multipolygon_winding(mp: MultiPolygon<f64>) -> MultiPolygon<f64> {
	MultiPolygon(mp.0.into_iter().map(normalize_polygon_winding).collect())
}

/// Returns `true` if a ring would round to a degenerate shape on the MVT
/// integer grid. A ring is degenerate when any of these hold:
///
/// - fewer than 3 vertices (after dropping the optional closing duplicate);
/// - fewer than 3 distinct integer-grid vertices (the encoder rounds each
///   coord half-away-from-zero, so sub-pixel rings collapse);
/// - the surveyor's signed area is within `WINDING_EPSILON` of zero
///   (collinear vertices).
///
/// Degenerate rings would still be written by the raw encoder but would not
/// render — and emitting them risks turning interior rings into orphan inners
/// at the decoder if the exterior is the degenerate one.
pub(super) fn ring_is_degenerate(coords: &[Coord<f64>]) -> bool {
	let n = if coords.len() >= 2 && coords.first() == coords.last() {
		coords.len() - 1
	} else {
		coords.len()
	};
	if n < 3 {
		return true;
	}
	let coords = &coords[..n];

	let mut seen = std::collections::HashSet::<(i64, i64)>::with_capacity(n);
	for c in coords {
		#[allow(clippy::cast_possible_truncation)]
		seen.insert((c.x.round() as i64, c.y.round() as i64));
		if seen.len() >= 3 {
			break;
		}
	}
	if seen.len() < 3 {
		return true;
	}

	ring_signed_double_area(coords).abs() < WINDING_EPSILON
}

/// Parses an MVT geometry command stream into a `Vec` of point sequences, one
/// per `MoveTo` boundary. Used by both [`VectorTileFeature::to_geometry`] and
/// the spec validator — they need exactly the same parsing semantics.
///
/// See <https://github.com/mapbox/vector-tile-spec/blob/master/2.1/README.md#43-geometry-encoding>.
pub(super) fn parse_geom_command_stream(geom_data: &Blob) -> Result<Vec<Vec<Coord<f64>>>> {
	let mut reader = ValueReaderSlice::new_le(geom_data.as_slice());

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

					#[allow(clippy::cast_precision_loss)]
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

	Ok(lines)
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

		writer
			.write_pbf_key(4, 2)
			.context("Failed to write PBF key for geometry data")?;
		writer
			.write_pbf_blob(&self.geom_data)
			.context("Failed to write geometry data")?;

		Ok(writer.into_blob())
	}

	pub fn to_geometry(&self) -> Result<Geometry<f64>> {
		let coordinates = parse_geom_command_stream(&self.geom_data)?;

		match self.geom_type {
			// Spec-compliant "no geometry" form: a feature with `geom_type=0`
			// (and, in practice, empty `geom_data`). Surfaces as an empty
			// `GeometryCollection` so the encoder can round-trip it back to
			// Unknown without inventing a fake type.
			GeomType::Unknown => Ok(Geometry::GeometryCollection(GeometryCollection(vec![]))),

			GeomType::MultiPoint => {
				// Empty MultiPoint data is the symmetric counterpart of the
				// encoder's degenerate-input case (see `from_geometry`). Treat
				// it as an empty MultiPoint rather than failing the decode.
				if coordinates.is_empty() {
					return Ok(Geometry::MultiPoint(MultiPoint(vec![])));
				}
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
				if coordinates.is_empty() {
					return Ok(Geometry::MultiLineString(MultiLineString(vec![])));
				}
				let lines = coordinates.into_iter().map(LineString::new).collect::<Vec<_>>();
				let g = Geometry::MultiLineString(MultiLineString(lines));
				validate(&g).context("Invalid MultiLineString")?;
				Ok(g)
			}

			GeomType::MultiPolygon => {
				// Empty MultiPolygon data is a legitimate intermediate state when
				// every ring of the original geometry was degenerate and the
				// encoder dropped the whole feature. Decode it as an empty
				// MultiPolygon rather than failing.
				if coordinates.is_empty() {
					return Ok(Geometry::MultiPolygon(MultiPolygon(vec![])));
				}
				let mut current_polygon: Vec<LineString<f64>> = Vec::new();
				let mut polygons: Vec<Polygon<f64>> = Vec::new();
				// Surface spec violations on first occurrence per feature. Repeated rings
				// in the same feature do not retrigger the warning to avoid flooding the
				// log when an entire tile is malformed.
				let mut bad_winding_warned = false;

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
							if !bad_winding_warned {
								log::warn!(
									"Dropping orphan inner ring with no preceding outer — \
									 likely indicates inverted polygon winding in source MVT \
									 (violates MVT 2.1 §4.3.3.3)"
								);
								bad_winding_warned = true;
							}
						} else {
							current_polygon.push(ring);
						}
					} else {
						log::debug!("Skipping polygon ring with zero area (collinear or degenerate vertices)");
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

	/// Like [`Self::to_geometry`] but tolerant of inverted polygon winding.
	///
	/// The strict decoder follows MVT 2.1 §4.3.3.3 literally: it classifies
	/// rings by signed area, dropping any negative-area ring that appears
	/// before a positive-area ring. That is correct for spec-conformant
	/// input but lossy when the source data uses the opposite winding
	/// convention — the real outer rings get dropped as "orphan inners",
	/// silently losing the geometry's shape.
	///
	/// This decoder detects per-feature inversion via the dominant ring-area
	/// sign (`total_pos_area >= total_neg_area`). When the convention is
	/// inverted, all rings are reversed *before* outer/inner classification.
	/// The result is correctly-shaped geometry regardless of which convention
	/// the source used.
	///
	/// Used by `vector_repair` in repair contexts where the operation's
	/// purpose is to fix malformed input. The default decoder
	/// ([`Self::to_geometry`]) remains strict.
	///
	/// For non-polygon types this delegates to [`Self::to_geometry`] — there
	/// is no winding rule for points or line strings.
	///
	/// # Caveats
	///
	/// The per-feature heuristic assumes uniform convention within a
	/// feature. A feature carrying a mix of correctly-wound and
	/// inversely-wound polygons would be processed as if all polygons
	/// followed the dominant convention; the others would end up with
	/// their winding flipped. Such mixed-convention features are
	/// pathological and not known to occur in real data.
	pub fn to_geometry_lenient(&self) -> Result<Geometry<f64>> {
		if self.geom_type == GeomType::MultiPolygon {
			self.decode_polygon_lenient()
		} else {
			self.to_geometry()
		}
	}

	/// `to_feature` counterpart of [`Self::to_geometry_lenient`]: builds a
	/// [`GeoFeature`] using the lenient geometry decoder, then attaches the
	/// id and decoded tags from `layer`.
	pub fn to_feature_lenient(&self, layer: &VectorTileLayer) -> Result<GeoFeature> {
		let mut feature = GeoFeature::new(
			self
				.to_geometry_lenient()
				.context("Failed to convert to geometry (lenient)")?,
		);
		if let Some(id) = self.id {
			feature.set_id(GeoValue::from(id));
		}
		feature.properties = layer.decode_tag_ids(&self.tag_ids)?;
		Ok(feature)
	}

	fn decode_polygon_lenient(&self) -> Result<Geometry<f64>> {
		let rings = parse_geom_command_stream(&self.geom_data)?;
		if rings.is_empty() {
			return Ok(Geometry::MultiPolygon(MultiPolygon(vec![])));
		}

		// Per-feature convention detection: outer rings typically have larger
		// absolute area than inner rings, so the side that contributes more
		// total area is the one carrying the outers.
		let areas: Vec<f64> = rings.iter().map(|r| ring_signed_double_area(r)).collect();
		let total_pos: f64 = areas.iter().filter(|&&a| a > WINDING_EPSILON).sum();
		let total_neg: f64 = areas.iter().filter(|&&a| a < -WINDING_EPSILON).map(|a| -a).sum();
		let outer_is_positive = total_pos >= total_neg;

		let mut current: Vec<LineString<f64>> = Vec::new();
		let mut polygons: Vec<Polygon<f64>> = Vec::new();
		let push = |rings: Vec<LineString<f64>>, polygons: &mut Vec<Polygon<f64>>| {
			if let Some((exterior, interiors)) = rings.split_first() {
				polygons.push(Polygon::new(exterior.clone(), interiors.to_vec()));
			}
		};

		for (mut ring_coords, area2) in rings.into_iter().zip(areas.iter().copied()) {
			if area2.abs() <= WINDING_EPSILON {
				// Degenerate ring — would be filtered by the encoder anyway.
				continue;
			}
			ensure!(ring_coords.len() >= 4, "polygon ring must have at least 4 points");
			ensure!(ring_coords.first() == ring_coords.last(), "polygon ring must be closed");

			// Rewind so positive area = outer once classification runs below.
			if !outer_is_positive {
				ring_coords.reverse();
			}
			let is_outer = if outer_is_positive { area2 > 0.0 } else { area2 < 0.0 };
			let ring = LineString::new(ring_coords);

			if is_outer {
				if !current.is_empty() {
					push(std::mem::take(&mut current), &mut polygons);
				}
				current.push(ring);
			} else if !current.is_empty() {
				current.push(ring);
			}
			// Inner rings with no preceding outer (still possible if the
			// inversion-detection picked the wrong convention) are dropped
			// here, same as the strict decoder.
		}

		if !current.is_empty() {
			push(current, &mut polygons);
		}
		Ok(Geometry::MultiPolygon(MultiPolygon(polygons)))
	}

	#[allow(clippy::too_many_lines)]
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
			if points.0.is_empty() {
				return Ok(writer.into_blob());
			}
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

			// Normalise ring winding to MVT 2.1 §4.3.3.3 before writing varints.
			// Callers can hand us geometry from any source (geojson, shapefile,
			// pipeline output) without worrying about winding; the on-disk MVT
			// always conforms to the spec.
			let polygons = normalize_multipolygon_winding(polygons);

			for polygon in polygons.0 {
				let (exterior, interiors) = polygon.into_inner();
				// Skip the whole polygon if its exterior is degenerate. Emitting
				// only the interiors would leave them as orphan inner rings in
				// the MVT stream, which the decoder drops — silent data loss.
				if ring_is_degenerate(&exterior.0) {
					continue;
				}
				write_ring(&mut writer, point0, &exterior)?;
				for interior in interiors {
					if ring_is_degenerate(&interior.0) {
						continue;
					}
					write_ring(&mut writer, point0, &interior)?;
				}
			}

			Ok(writer.into_blob())
		}

		let (mut geom_type, geom_data) = match geometry {
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
			// Empty GeometryCollection is the in-memory form of an Unknown
			// feature with no geometry. Non-empty collections aren't
			// representable in MVT and stay rejected.
			Geometry::GeometryCollection(gc) if gc.0.is_empty() => (GeomType::Unknown, Blob::new_empty()),
			Geometry::GeometryCollection(_) => bail!("MVT encoding of non-empty GeometryCollection is not supported"),
		};

		// MVT 2.1 §4.3 requires Point/LineString/Polygon features to carry at
		// least one point/vertex/ring. If the writer produced an empty command
		// stream (empty input, or every ring degenerate), downgrade to Unknown
		// so the encoded feature stays spec-compliant — an Unknown feature
		// with empty geom_data is the canonical "no geometry" form.
		if geom_data.is_empty() {
			geom_type = GeomType::Unknown;
		}

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

	/// Regression for the MVT 2.1 §4.2 requirement that every feature MUST
	/// carry the geometry field (PBF tag 4). Earlier versions skipped the
	/// field when `geom_data` was empty, which broke MapLibre interop.
	#[test]
	fn to_blob_always_emits_geometry_field() -> Result<()> {
		use versatiles_core::io::ValueReaderSlice;
		let cases: Vec<Geometry<f64>> = vec![
			Geometry::Point(Point::new(1.0, 2.0)),
			Geometry::MultiPoint(MultiPoint(vec![])),
			Geometry::MultiLineString(MultiLineString(vec![])),
			Geometry::MultiPolygon(MultiPolygon(vec![])),
		];
		for g in cases {
			let label = format!("{g:?}");
			let feat = VectorTileFeature::from_geometry(None, vec![], g)?;
			let blob = feat.to_blob()?;
			let mut reader = ValueReaderSlice::new_le(blob.as_slice());
			let decoded = VectorTileFeature::read(&mut reader).expect(&label);
			// Re-parsing succeeds only if field 4 was emitted (decoder bails
			// otherwise — no other field carries geom_data).
			assert_eq!(
				decoded.geom_data.as_slice(),
				feat.geom_data.as_slice(),
				"geom_data round-trip mismatch for {label}"
			);
		}
		Ok(())
	}

	/// Empty input geometries must be encoded as `Unknown` with empty
	/// `geom_data`, the spec-compliant "no geometry" form.
	#[test]
	fn from_geometry_downgrades_empty_to_unknown() -> Result<()> {
		let cases: Vec<Geometry<f64>> = vec![
			Geometry::MultiPoint(MultiPoint(vec![])),
			Geometry::LineString(LineString::new(vec![])),
			Geometry::MultiLineString(MultiLineString(vec![])),
			Geometry::MultiPolygon(MultiPolygon(vec![])),
		];
		for g in cases {
			let label = format!("{g:?}");
			let feat = VectorTileFeature::from_geometry(None, vec![], g)?;
			assert_eq!(feat.geom_type, GeomType::Unknown, "{label}");
			assert!(feat.geom_data.is_empty(), "{label}");
		}
		Ok(())
	}

	/// All-degenerate polygons produce empty `geom_data`; the encoder
	/// downgrades them to `Unknown` rather than emitting `MultiPolygon` with
	/// no rings (which decoders may interpret inconsistently).
	#[test]
	fn from_geometry_downgrades_all_degenerate_polygon_to_unknown() -> Result<()> {
		let collinear = polygon_from(&[vec![[0, 0], [1, 1], [2, 2], [0, 0]]]);
		let feat = VectorTileFeature::from_geometry(None, vec![], Geometry::Polygon(collinear))?;
		assert_eq!(feat.geom_type, GeomType::Unknown);
		assert!(feat.geom_data.is_empty());
		Ok(())
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

	/// Builds a polygon-typed `VectorTileFeature` whose `geom_data` encodes the
	/// given rings literally, in the given order and winding. Unlike
	/// `from_geometry`, this does not normalise or validate the input, so it can
	/// be used to construct spec-violating fixtures.
	#[cfg(test)]
	fn raw_polygon_feature(rings: &[Vec<(i32, i32)>]) -> VectorTileFeature {
		use versatiles_core::io::{ValueWriter, ValueWriterBlob};
		let mut writer = ValueWriterBlob::new_le();
		let mut prev = (0i64, 0i64);
		for ring in rings {
			assert!(ring.len() >= 3, "ring needs at least 3 vertices");
			let (fx, fy) = ring[0];
			let (ix, iy) = (i64::from(fx), i64::from(fy));
			writer.write_varint((1 << 3) | 0x1).unwrap(); // MoveTo, count=1
			writer.write_svarint(ix - prev.0).unwrap();
			writer.write_svarint(iy - prev.1).unwrap();
			prev = (ix, iy);

			let rest = ring.len() - 1;
			writer.write_varint(((rest as u64) << 3) | 0x2).unwrap(); // LineTo, count=rest
			for &(fx, fy) in &ring[1..] {
				let (ix, iy) = (i64::from(fx), i64::from(fy));
				writer.write_svarint(ix - prev.0).unwrap();
				writer.write_svarint(iy - prev.1).unwrap();
				prev = (ix, iy);
			}
			writer.write_varint(7).unwrap(); // ClosePath
		}
		VectorTileFeature {
			id: None,
			tag_ids: vec![],
			geom_type: GeomType::MultiPolygon,
			geom_data: writer.into_blob(),
		}
	}

	/// A feature whose rings all classify as inner (no preceding outer) — i.e.
	/// the inverted-winding case from `landcover-vectors#3`. The decoder must
	/// drop them silently (returning an empty MultiPolygon) and warn once.
	#[test]
	fn orphan_inner_rings_decode_to_empty_multipolygon() -> Result<()> {
		// Two CCW (positive area) rings — outer in spec, but if a producer
		// emits them as the *first* rings in a feature without preceding CW
		// outers, the strict decoder treats them as outer rings, not orphans.
		// To actually exercise the orphan-inner path we need CW (negative area)
		// rings at the start of the feature.
		let inner_a = vec![(0, 0), (0, 100), (100, 100), (100, 0), (0, 0)]; // CW (screen) → negative area
		let inner_b = vec![(200, 200), (200, 300), (300, 300), (300, 200), (200, 200)];
		let feature = raw_polygon_feature(&[inner_a, inner_b]);

		let geom = feature.to_geometry()?;
		match geom {
			Geometry::MultiPolygon(mp) => {
				assert!(
					mp.0.is_empty(),
					"orphan inner rings must be dropped; got {} polygon(s)",
					mp.0.len()
				);
			}
			other => panic!("expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	/// A feature whose only rings have zero area (collinear vertices). The
	/// decoder must drop them silently at debug-log level.
	#[test]
	fn zero_area_rings_decode_to_empty_multipolygon() -> Result<()> {
		let collinear = vec![(0, 0), (10, 10), (20, 20), (0, 0)];
		let feature = raw_polygon_feature(&[collinear]);
		let geom = feature.to_geometry()?;
		match geom {
			Geometry::MultiPolygon(mp) => assert!(mp.0.is_empty()),
			other => panic!("expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	// ── to_geometry_lenient ───────────────────────────────────────────────

	/// Inverted-winding polygon-with-hole (outer CCW screen / negative area,
	/// inner CW screen / positive area) — the landcover-vectors#3 pattern.
	/// Strict decode drops the outer; lenient decode detects inversion via
	/// `total_pos < total_neg` (the outer's |area| dominates the inner's)
	/// and rewinds all rings before classification.
	#[test]
	fn lenient_decode_recovers_inverted_winding_polygon_with_hole() -> Result<()> {
		let outer_ccw = vec![(0, 0), (0, 100), (100, 100), (100, 0)]; // CCW → -10000
		let inner_cw = vec![(20, 20), (60, 20), (60, 60), (20, 60)]; // CW  → +1600
		let feature = raw_polygon_feature(&[outer_ccw, inner_cw]);

		// Sanity: strict decode loses the outer.
		let strict = feature.to_geometry()?;
		match strict {
			Geometry::MultiPolygon(mp) => {
				assert_eq!(mp.0.len(), 1, "strict: one (wrong) polygon");
				assert!(mp.0[0].interiors().is_empty(), "strict: hole lost");
			}
			other => panic!("expected MultiPolygon, got {other:?}"),
		}

		// Lenient decode preserves the shape: one polygon, one interior.
		let lenient = feature.to_geometry_lenient()?;
		match lenient {
			Geometry::MultiPolygon(mp) => {
				assert_eq!(mp.0.len(), 1, "lenient: one polygon");
				assert_eq!(mp.0[0].interiors().len(), 1, "lenient: hole preserved");
				assert!(
					ring_signed_double_area(&mp.0[0].exterior().0) > 0.0,
					"lenient: exterior must be normalised to positive area"
				);
				assert!(
					ring_signed_double_area(&mp.0[0].interiors()[0].0) < 0.0,
					"lenient: interior must be normalised to negative area"
				);
			}
			other => panic!("expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	/// Standard MVT 2.1 winding round-trips unchanged through the lenient
	/// decoder (no regression for well-formed input).
	#[test]
	fn lenient_decode_preserves_standard_winding() -> Result<()> {
		let outer_cw = vec![(0, 0), (100, 0), (100, 100), (0, 100)]; // CW → +10000
		let inner_ccw = vec![(20, 20), (20, 60), (60, 60), (60, 20)]; // CCW → -1600
		let feature = raw_polygon_feature(&[outer_cw, inner_ccw]);
		let geom = feature.to_geometry_lenient()?;
		match geom {
			Geometry::MultiPolygon(mp) => {
				assert_eq!(mp.0.len(), 1);
				assert_eq!(mp.0[0].interiors().len(), 1);
			}
			other => panic!("expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	/// A polygon-feature whose rings are all degenerate should decode (under
	/// the lenient decoder) to an empty MultiPolygon, mirroring the strict
	/// decoder's empty-data behaviour.
	#[test]
	fn lenient_decode_empty_when_all_rings_degenerate() -> Result<()> {
		let collinear = vec![(0, 0), (5, 5), (10, 10), (0, 0)];
		let feature = raw_polygon_feature(&[collinear]);
		let geom = feature.to_geometry_lenient()?;
		match geom {
			Geometry::MultiPolygon(mp) => assert!(mp.0.is_empty()),
			other => panic!("expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	// ── normalize_polygon_winding ─────────────────────────────────────────

	/// Returns the vertices of a ring as `(x, y)` tuples, dropping the closing
	/// duplicate so equality assertions are order-only.
	fn ring_pts(ls: &LineString<f64>) -> Vec<(f64, f64)> {
		ls.0.iter().map(|c| (c.x, c.y)).collect()
	}

	/// Polygon already in MVT 2.1 winding (outer CW screen = positive area,
	/// inner CCW screen = negative area) is left unchanged.
	#[test]
	fn normalize_polygon_winding_noop_for_correct_input() {
		// Outer: (0,0)-(4,0)-(4,4)-(0,4) — CW in screen-Y → positive area2.
		let outer = ls_from(&[[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]]);
		// Inner: (1,1)-(1,3)-(3,3)-(3,1) — CCW in screen-Y → negative area2.
		let inner = ls_from(&[[1, 1], [1, 3], [3, 3], [3, 1], [1, 1]]);
		let original = Polygon::new(outer.clone(), vec![inner.clone()]);

		let normalized = normalize_polygon_winding(original);

		assert_eq!(ring_pts(normalized.exterior()), ring_pts(&outer));
		assert_eq!(normalized.interiors().len(), 1);
		assert_eq!(ring_pts(&normalized.interiors()[0]), ring_pts(&inner));
	}

	/// Inverted outer ring (CCW screen, negative area) is reversed; inner is
	/// left alone.
	#[test]
	fn normalize_polygon_winding_reverses_inverted_outer() {
		let outer_inverted = ls_from(&[[0, 0], [0, 4], [4, 4], [4, 0], [0, 0]]); // CCW → negative area
		let inner = ls_from(&[[1, 1], [1, 3], [3, 3], [3, 1], [1, 1]]); // already CCW → negative area
		let original = Polygon::new(outer_inverted.clone(), vec![inner.clone()]);

		let normalized = normalize_polygon_winding(original);

		// Exterior should now be CW (positive area).
		assert!(ring_signed_double_area(&normalized.exterior().0) > 0.0);
		// And it should be the *reverse* of the input.
		let mut reversed = ring_pts(&outer_inverted);
		reversed.reverse();
		assert_eq!(ring_pts(normalized.exterior()), reversed);
		// Interior was already correct → unchanged.
		assert_eq!(ring_pts(&normalized.interiors()[0]), ring_pts(&inner));
	}

	/// Inverted inner ring (CW screen, positive area) is reversed; outer is
	/// left alone.
	#[test]
	fn normalize_polygon_winding_reverses_inverted_inner() {
		let outer = ls_from(&[[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]]); // CW → positive area
		let inner_inverted = ls_from(&[[1, 1], [3, 1], [3, 3], [1, 3], [1, 1]]); // CW → positive area
		let original = Polygon::new(outer.clone(), vec![inner_inverted.clone()]);

		let normalized = normalize_polygon_winding(original);

		assert_eq!(ring_pts(normalized.exterior()), ring_pts(&outer));
		// Interior should now have negative area.
		assert!(ring_signed_double_area(&normalized.interiors()[0].0) < 0.0);
		let mut reversed = ring_pts(&inner_inverted);
		reversed.reverse();
		assert_eq!(ring_pts(&normalized.interiors()[0]), reversed);
	}

	/// Degenerate ring (area ≈ 0) is left unchanged regardless of input.
	#[test]
	fn normalize_polygon_winding_leaves_degenerate_alone() {
		// Collinear outer — area is exactly zero.
		let outer_collinear = ls_from(&[[0, 0], [1, 1], [2, 2], [0, 0]]);
		let original = Polygon::new(outer_collinear.clone(), vec![]);

		let normalized = normalize_polygon_winding(original);

		assert_eq!(ring_pts(normalized.exterior()), ring_pts(&outer_collinear));
	}

	/// Round-trip a polygon-with-hole whose rings are *both* inverted relative
	/// to MVT 2.1. Before C2 the encoder would emit the inverted bytes,
	/// `to_geometry` would classify the (then-orphan) inner rings and lose the
	/// hole. After C2 the encoder rewinds first, so the on-disk MVT is
	/// conformant and the hole survives the round-trip.
	#[test]
	fn from_geometry_normalises_inverted_winding() -> Result<()> {
		let outer_inverted = ls_from(&[[0, 0], [0, 4], [4, 4], [4, 0], [0, 0]]); // CCW → negative area
		let inner_inverted = ls_from(&[[1, 1], [3, 1], [3, 3], [1, 3], [1, 1]]); // CW → positive area
		let bad = Polygon::new(outer_inverted, vec![inner_inverted]);

		let feature = VectorTileFeature::from_geometry(None, vec![], Geometry::Polygon(bad))?;
		let decoded = feature.to_geometry()?;

		match decoded {
			Geometry::MultiPolygon(mp) => {
				assert_eq!(mp.0.len(), 1, "exactly one polygon survives the round-trip");
				assert_eq!(
					mp.0[0].interiors().len(),
					1,
					"hole must survive — was lost pre-fix because of inverted winding"
				);
				assert!(ring_signed_double_area(&mp.0[0].exterior().0) > 0.0);
				assert!(ring_signed_double_area(&mp.0[0].interiors()[0].0) < 0.0);
			}
			other => panic!("expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	// ── ring_is_degenerate ────────────────────────────────────────────────

	#[test]
	fn ring_is_degenerate_too_few_vertices() {
		assert!(ring_is_degenerate(&coords(&[(0.0, 0.0), (1.0, 0.0), (0.0, 0.0)]))); // 2 unique + closing dup
		assert!(ring_is_degenerate(&coords(&[(0.0, 0.0), (1.0, 0.0)])));
		assert!(ring_is_degenerate(&[]));
	}

	#[test]
	fn ring_is_degenerate_collinear() {
		// All three points on a line — zero area.
		assert!(ring_is_degenerate(&coords(&[
			(0.0, 0.0),
			(1.0, 1.0),
			(2.0, 2.0),
			(0.0, 0.0),
		])));
	}

	#[test]
	fn ring_is_degenerate_subpixel_collapses_to_grid_point() {
		// Three vertices that all round to (0, 0) at the integer grid.
		assert!(ring_is_degenerate(&coords(&[
			(0.0, 0.0),
			(0.1, 0.1),
			(-0.2, 0.2),
			(0.0, 0.0),
		])));
	}

	#[test]
	fn ring_is_degenerate_normal_triangle_is_fine() {
		assert!(!ring_is_degenerate(&coords(&[
			(0.0, 0.0),
			(4.0, 0.0),
			(0.0, 4.0),
			(0.0, 0.0),
		])));
	}

	/// A polygon with a degenerate exterior must be dropped wholesale by the
	/// encoder — interior rings would otherwise become orphan inners and the
	/// decoder would silently drop them. The resulting feature carries no
	/// geometry, so it's encoded as `Unknown` with empty `geom_data`.
	#[test]
	fn encoder_drops_polygon_with_degenerate_exterior() -> Result<()> {
		// Exterior: three collinear points → zero area, degenerate.
		// Interior: a valid (CCW = negative area) ring that would normally
		// survive as a hole.
		let degenerate_exterior = ls_from(&[[0, 0], [1, 1], [2, 2], [0, 0]]);
		let valid_hole = ls_from(&[[3, 3], [3, 5], [5, 5], [5, 3], [3, 3]]);
		let poly = Polygon::new(degenerate_exterior, vec![valid_hole]);

		let feature = VectorTileFeature::from_geometry(None, vec![], Geometry::Polygon(poly))?;
		assert_eq!(feature.geom_type, GeomType::Unknown);
		assert!(feature.geom_data.is_empty());
		Ok(())
	}

	/// A polygon with a valid exterior and a degenerate interior must encode
	/// the exterior and drop the bad interior — the polygon itself survives.
	#[test]
	fn encoder_drops_degenerate_interior_but_keeps_exterior() -> Result<()> {
		let exterior = ls_from(&[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]); // CW screen, positive area
		let degenerate_hole = ls_from(&[[3, 3], [4, 4], [5, 5], [3, 3]]); // collinear
		let valid_hole = ls_from(&[[6, 6], [6, 8], [8, 8], [8, 6], [6, 6]]); // CCW screen, negative area
		let poly = Polygon::new(exterior, vec![degenerate_hole, valid_hole]);

		let feature = VectorTileFeature::from_geometry(None, vec![], Geometry::Polygon(poly))?;
		let decoded = feature.to_geometry()?;
		match decoded {
			Geometry::MultiPolygon(mp) => {
				assert_eq!(mp.0.len(), 1, "polygon survives");
				assert_eq!(
					mp.0[0].interiors().len(),
					1,
					"only the valid hole survives — got {}",
					mp.0[0].interiors().len()
				);
			}
			other => panic!("expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	/// In a `MultiPolygon`, the degenerate polygons drop out and the valid
	/// ones survive — each is decided independently.
	#[test]
	fn encoder_drops_degenerate_polygons_from_multipolygon() -> Result<()> {
		let valid = Polygon::new(ls_from(&[[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]]), vec![]);
		let bad = Polygon::new(ls_from(&[[10, 10], [11, 11], [12, 12], [10, 10]]), vec![]);
		let mp = MultiPolygon(vec![valid, bad]);

		let feature = VectorTileFeature::from_geometry(None, vec![], Geometry::MultiPolygon(mp))?;
		let decoded = feature.to_geometry()?;
		match decoded {
			Geometry::MultiPolygon(mp) => {
				assert_eq!(mp.0.len(), 1, "only the valid polygon survives");
			}
			other => panic!("expected MultiPolygon, got {other:?}"),
		}
		Ok(())
	}

	/// `normalize_multipolygon_winding` decides per polygon — a mixed-input
	/// `MultiPolygon` ends up with each polygon individually conformant.
	#[test]
	fn normalize_multipolygon_winding_decides_per_polygon() {
		// Polygon 1: outer already CW (correct), no holes.
		let p1 = Polygon::new(ls_from(&[[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]]), vec![]);
		// Polygon 2: outer inverted (CCW), inverted hole (CW).
		let p2 = Polygon::new(
			ls_from(&[[10, 10], [10, 14], [14, 14], [14, 10], [10, 10]]),
			vec![ls_from(&[[11, 11], [13, 11], [13, 13], [11, 13], [11, 11]])],
		);

		let mp = MultiPolygon(vec![p1.clone(), p2]);
		let normalized = normalize_multipolygon_winding(mp);

		// First polygon unchanged.
		assert_eq!(ring_pts(normalized.0[0].exterior()), ring_pts(p1.exterior()));
		// Second polygon: both rings flipped.
		assert!(ring_signed_double_area(&normalized.0[1].exterior().0) > 0.0);
		assert!(ring_signed_double_area(&normalized.0[1].interiors()[0].0) < 0.0);
	}
}
