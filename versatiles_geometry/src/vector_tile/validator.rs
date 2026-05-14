//! MVT spec validator.
//!
//! Walks a [`VectorTile`] and reports anything that violates MVT 2.1 in a way
//! the rest of the pipeline would tolerate silently. The pipeline's policy is
//! "fail soft": orphan inner rings get dropped, degenerate rings disappear at
//! encode time, unknown geometry types come back as errors per-feature. None
//! of those surface to the operator without trace-level logging. The
//! validator gives you a structured report instead.
//!
//! ## What gets reported
//!
//! - **`OrphanInnerRing`**: a polygon ring with negative surveyor area
//!   (counter-clockwise in screen-Y) appears before any positive-area ring in
//!   the same feature. The strict decoder drops it; usually a symptom of
//!   inverted winding in the source data.
//! - **`DegenerateRing`**: a ring that rounds to fewer than 3 distinct
//!   integer-grid points (`SubPixel`), has fewer than 3 vertices
//!   (`TooFewVertices`), or has zero/near-zero surveyor area (`Collinear`).
//! - **`UnknownGeometryType`**: feature has geometry type 0 ("Unknown") but
//!   carries non-empty `geom_data` — the spec defines type 0 but does not
//!   assign it a shape, so attached data is ambiguous.
//! - **`EmptyGeometryForType`**: feature carries a non-Unknown geometry type
//!   but the command stream parses to zero coordinates — violates MVT 2.1
//!   §4.3 (Point/LineString/Polygon all require at least one element).
//! - **`MalformedCommandStream`**: the geometry command stream could not be
//!   parsed (bad varint, unknown command, ClosePath on empty linestring,
//!   …). The string carries the parser's error message.
//!
//! ## What does NOT get reported
//!
//! - Layers with `extent == 0` (the encoder skips them by default).
//! - Features with `geom_type == Unknown` *and* empty `geom_data` — that's
//!   the canonical "no geometry" form (MVT 2.1 allows it; the encoder emits
//!   it for inputs that collapse to nothing).
//! - Inverted polygon winding *per se*, when the polygon parses cleanly as
//!   "one outer + zero or more inners". The orphan-inner case is the
//!   observable symptom.

use super::VectorTile;
use super::feature::{WINDING_EPSILON, parse_geom_command_stream, ring_signed_double_area};
use super::geometry_type::GeomType;
use geo_types::Coord;

/// A single MVT spec violation, located by layer name + feature index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
	pub layer: String,
	pub feature_index: usize,
	pub kind: IssueKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueKind {
	/// A polygon ring with negative surveyor area appeared before any
	/// positive-area ring in this feature. The strict decoder drops it as an
	/// orphan; usually indicates inverted polygon winding upstream.
	OrphanInnerRing,
	/// A ring that would not render after encoding to MVT integer-grid
	/// coordinates. The wrapped reason gives the precise failure mode.
	DegenerateRing(DegenerateReason),
	/// Feature has geometry type 0 ("Unknown") but carries non-empty
	/// `geom_data`. The spec defines type 0 but doesn't assign it a shape,
	/// so the attached data is ambiguous.
	UnknownGeometryType,
	/// Feature has a non-Unknown geometry type but the command stream parses
	/// to zero coordinates. Violates MVT 2.1 §4.3 which requires at least one
	/// point/vertex/ring for each typed geometry. The wrapped value is the
	/// declared type.
	EmptyGeometryForType(GeomType),
	/// The MVT geometry command stream could not be parsed. The string is the
	/// parser's `anyhow` error chain, joined.
	MalformedCommandStream(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegenerateReason {
	/// Fewer than 3 vertices in the ring (after dropping the optional closing
	/// duplicate).
	TooFewVertices,
	/// 3+ vertices but they round to fewer than 3 distinct integer-grid
	/// points. Sub-pixel rings.
	SubPixel,
	/// 3+ distinct grid points but signed surveyor area is within
	/// `WINDING_EPSILON` of zero. Collinear vertices.
	Collinear,
}

/// Validate every feature in `tile` and collect the spec violations found.
/// Returns an empty `Vec` when the tile is conformant.
#[must_use]
pub fn validate_tile(tile: &VectorTile) -> Vec<ValidationIssue> {
	let mut issues = Vec::new();
	for layer in &tile.layers {
		for (feature_index, feature) in layer.features.iter().enumerate() {
			validate_feature(&layer.name, feature_index, feature, &mut issues);
		}
	}
	issues
}

fn validate_feature(
	layer: &str,
	feature_index: usize,
	feature: &super::feature::VectorTileFeature,
	issues: &mut Vec<ValidationIssue>,
) {
	let push = |kind: IssueKind, issues: &mut Vec<ValidationIssue>| {
		issues.push(ValidationIssue {
			layer: layer.to_string(),
			feature_index,
			kind,
		});
	};

	if feature.geom_type == GeomType::Unknown {
		// (Unknown, empty) is the spec-compliant "no geometry" form — silent.
		// (Unknown, non-empty) is ambiguous — flag.
		if !feature.geom_data.is_empty() {
			push(IssueKind::UnknownGeometryType, issues);
		}
		return;
	}

	let rings = match parse_geom_command_stream(&feature.geom_data) {
		Ok(rings) => rings,
		Err(e) => {
			push(IssueKind::MalformedCommandStream(format!("{e:#}")), issues);
			return;
		}
	};

	// A typed feature with no parsed coordinates violates MVT 2.1 §4.3.
	// The encoder downgrades such features to Unknown, so this only fires
	// for inbound tiles produced by other tools.
	if rings.iter().all(Vec::is_empty) {
		push(IssueKind::EmptyGeometryForType(feature.geom_type), issues);
		return;
	}

	match feature.geom_type {
		GeomType::MultiPolygon => check_polygon_rings(layer, feature_index, &rings, issues),
		// Points and line strings: no winding rules, just degeneracy makes sense.
		// Line strings need ≥2 distinct grid vertices, but the encoder already
		// rejects shorter inputs, so we only flag here if the data is malformed
		// (caught above) or truly degenerate.
		GeomType::MultiLineString => {
			for ring in &rings {
				if let Some(reason) = degeneracy_reason_for_linestring(ring) {
					push(IssueKind::DegenerateRing(reason), issues);
				}
			}
		}
		GeomType::MultiPoint | GeomType::Unknown => {}
	}
}

fn check_polygon_rings(
	layer: &str,
	feature_index: usize,
	rings: &[Vec<Coord<f64>>],
	issues: &mut Vec<ValidationIssue>,
) {
	let mut saw_outer = false;
	for ring in rings {
		// Degeneracy is a structural issue independent of winding.
		if let Some(reason) = degeneracy_reason(ring) {
			issues.push(ValidationIssue {
				layer: layer.to_string(),
				feature_index,
				kind: IssueKind::DegenerateRing(reason),
			});
			continue;
		}

		let area2 = ring_signed_double_area(ring);
		if area2 > WINDING_EPSILON {
			// Outer ring.
			saw_outer = true;
		} else if area2 < -WINDING_EPSILON && !saw_outer {
			// Negative area before any outer → orphan inner.
			issues.push(ValidationIssue {
				layer: layer.to_string(),
				feature_index,
				kind: IssueKind::OrphanInnerRing,
			});
		}
		// (negative area after an outer = legitimate inner, no issue)
	}
}

/// Like `ring_is_degenerate` from `feature.rs` but returns the specific reason
/// so the validator can surface it. Returns `None` for non-degenerate rings.
fn degeneracy_reason(coords: &[Coord<f64>]) -> Option<DegenerateReason> {
	let n = if coords.len() >= 2 && coords.first() == coords.last() {
		coords.len() - 1
	} else {
		coords.len()
	};
	if n < 3 {
		return Some(DegenerateReason::TooFewVertices);
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
		return Some(DegenerateReason::SubPixel);
	}

	if ring_signed_double_area(coords).abs() < WINDING_EPSILON {
		return Some(DegenerateReason::Collinear);
	}
	None
}

/// Degeneracy check for line strings: needs at least 2 distinct grid points to
/// be visible. Returns the reason if degenerate, else `None`.
fn degeneracy_reason_for_linestring(coords: &[Coord<f64>]) -> Option<DegenerateReason> {
	if coords.len() < 2 {
		return Some(DegenerateReason::TooFewVertices);
	}
	let mut seen = std::collections::HashSet::<(i64, i64)>::with_capacity(coords.len());
	for c in coords {
		#[allow(clippy::cast_possible_truncation)]
		seen.insert((c.x.round() as i64, c.y.round() as i64));
		if seen.len() >= 2 {
			return None;
		}
	}
	Some(DegenerateReason::SubPixel)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::vector_tile::VectorTileLayer;
	use crate::vector_tile::feature::VectorTileFeature;
	use versatiles_core::Blob;
	use versatiles_core::io::{ValueWriter, ValueWriterBlob};

	/// Build a polygon-typed feature whose `geom_data` is the literal rings —
	/// bypasses the encoder's winding normalisation and degeneracy filtering,
	/// so we can construct deliberately spec-violating fixtures.
	fn raw_polygon_feature(rings: &[Vec<(i32, i32)>]) -> VectorTileFeature {
		raw_feature(GeomType::MultiPolygon, rings)
	}

	fn raw_feature(geom_type: GeomType, rings: &[Vec<(i32, i32)>]) -> VectorTileFeature {
		let mut writer = ValueWriterBlob::new_le();
		let mut prev = (0i64, 0i64);
		for ring in rings {
			assert!(!ring.is_empty());
			let (fx, fy) = ring[0];
			let (ix, iy) = (i64::from(fx), i64::from(fy));
			writer.write_varint((1 << 3) | 0x1).unwrap(); // MoveTo count=1
			writer.write_svarint(ix - prev.0).unwrap();
			writer.write_svarint(iy - prev.1).unwrap();
			prev = (ix, iy);
			let rest = ring.len() - 1;
			if rest > 0 {
				writer.write_varint(((rest as u64) << 3) | 0x2).unwrap();
				for &(fx, fy) in &ring[1..] {
					let (ix, iy) = (i64::from(fx), i64::from(fy));
					writer.write_svarint(ix - prev.0).unwrap();
					writer.write_svarint(iy - prev.1).unwrap();
					prev = (ix, iy);
				}
			}
			if geom_type == GeomType::MultiPolygon {
				writer.write_varint(7).unwrap(); // ClosePath
			}
		}
		VectorTileFeature {
			id: None,
			tag_ids: vec![],
			geom_type,
			geom_data: writer.into_blob(),
		}
	}

	fn layer_with_features(name: &str, features: Vec<VectorTileFeature>) -> VectorTileLayer {
		let mut layer = VectorTileLayer::new(name.to_string(), 4096, 1);
		layer.features = features;
		layer
	}

	fn tile_with_layer(layer: VectorTileLayer) -> VectorTile {
		VectorTile::new(vec![layer])
	}

	#[test]
	fn clean_tile_has_no_issues() {
		// CW screen = positive area = outer ring.
		let outer = vec![(0, 0), (4, 0), (4, 4), (0, 4)];
		let feature = raw_polygon_feature(&[outer]);
		let tile = tile_with_layer(layer_with_features("l", vec![feature]));
		assert!(validate_tile(&tile).is_empty());
	}

	#[test]
	fn detects_orphan_inner_ring() {
		// CCW screen = negative area = orphan inner (no preceding outer).
		let inner_first = vec![(0, 0), (0, 4), (4, 4), (4, 0)];
		let feature = raw_polygon_feature(&[inner_first]);
		let tile = tile_with_layer(layer_with_features("l", vec![feature]));
		let issues = validate_tile(&tile);
		assert_eq!(issues.len(), 1);
		assert_eq!(issues[0].kind, IssueKind::OrphanInnerRing);
		assert_eq!(issues[0].layer, "l");
		assert_eq!(issues[0].feature_index, 0);
	}

	#[test]
	fn detects_inverted_winding_landcover_pattern() {
		// landcover-vector pattern: outer CCW (negative area), inner CW (positive area).
		// Decoder treats the negative-area first ring as orphan inner.
		let outer_inverted = vec![(0, 0), (0, 100), (100, 100), (100, 0)];
		let inner_inverted = vec![(20, 20), (80, 20), (80, 80), (20, 80)];
		let feature = raw_polygon_feature(&[outer_inverted, inner_inverted]);
		let tile = tile_with_layer(layer_with_features("landcover", vec![feature]));
		let issues = validate_tile(&tile);
		assert_eq!(issues.len(), 1, "exactly one orphan-inner reported");
		assert_eq!(issues[0].kind, IssueKind::OrphanInnerRing);
	}

	#[test]
	fn detects_collinear_degenerate_ring() {
		let collinear = vec![(0, 0), (10, 10), (20, 20)]; // straight line
		let feature = raw_polygon_feature(&[collinear]);
		let tile = tile_with_layer(layer_with_features("l", vec![feature]));
		let issues = validate_tile(&tile);
		assert_eq!(issues.len(), 1);
		assert_eq!(issues[0].kind, IssueKind::DegenerateRing(DegenerateReason::Collinear));
	}

	#[test]
	fn detects_sub_pixel_degenerate_ring() {
		// All three vertices round to (0, 0) on the integer grid.
		// (raw_polygon_feature uses i32 coords, so to exercise the sub-pixel path
		// we synthesise the geom_data with deltas that round to the same point.)
		let mut writer = ValueWriterBlob::new_le();
		writer.write_varint((1 << 3) | 0x1).unwrap();
		writer.write_svarint(0).unwrap();
		writer.write_svarint(0).unwrap();
		writer.write_varint((2 << 3) | 0x2).unwrap();
		writer.write_svarint(0).unwrap(); // (0,0)
		writer.write_svarint(0).unwrap();
		writer.write_svarint(0).unwrap(); // (0,0)
		writer.write_svarint(0).unwrap();
		writer.write_varint(7).unwrap(); // ClosePath
		let feature = VectorTileFeature {
			id: None,
			tag_ids: vec![],
			geom_type: GeomType::MultiPolygon,
			geom_data: writer.into_blob(),
		};
		let tile = tile_with_layer(layer_with_features("l", vec![feature]));
		let issues = validate_tile(&tile);
		assert_eq!(issues.len(), 1);
		// Three identical vertices: the helper trims the closing duplicate,
		// leaves 3 vertices, all of which round to the same grid point.
		// That's TooFewVertices not SubPixel — wait, actually 3 separate
		// vertices that round to the same point IS SubPixel. Let me check.
		// (TooFewVertices fires when n < 3 after trimming. Here n = 3.
		//  SubPixel fires when distinct grid points < 3. Here 1 distinct.)
		assert_eq!(issues[0].kind, IssueKind::DegenerateRing(DegenerateReason::SubPixel));
	}

	#[test]
	fn unknown_with_empty_data_is_not_flagged() {
		// Canonical "no geometry" form — spec-compliant per MVT 2.1 §4.2.
		let feature = VectorTileFeature {
			id: None,
			tag_ids: vec![],
			geom_type: GeomType::Unknown,
			geom_data: Blob::new_empty(),
		};
		let tile = tile_with_layer(layer_with_features("l", vec![feature]));
		assert!(validate_tile(&tile).is_empty());
	}

	#[test]
	fn detects_unknown_geometry_with_attached_data() {
		// Type 0 + non-empty data → ambiguous (spec defines no shape for 0).
		let feature = VectorTileFeature {
			id: None,
			tag_ids: vec![],
			geom_type: GeomType::Unknown,
			geom_data: Blob::from(vec![0x09, 0x00, 0x00]),
		};
		let tile = tile_with_layer(layer_with_features("l", vec![feature]));
		let issues = validate_tile(&tile);
		assert_eq!(issues.len(), 1);
		assert_eq!(issues[0].kind, IssueKind::UnknownGeometryType);
	}

	#[test]
	fn detects_typed_feature_with_empty_geom_data() {
		// MVT 2.1 §4.3: typed features require at least one element. Empty
		// command stream on a typed feature is the spec violation our own
		// encoder downgrades to Unknown; we still surface it for inbound
		// tiles from other producers.
		for geom_type in [GeomType::MultiPoint, GeomType::MultiLineString, GeomType::MultiPolygon] {
			let feature = VectorTileFeature {
				id: None,
				tag_ids: vec![],
				geom_type,
				geom_data: Blob::new_empty(),
			};
			let tile = tile_with_layer(layer_with_features("l", vec![feature]));
			let issues = validate_tile(&tile);
			assert_eq!(issues.len(), 1, "type={geom_type:?}");
			assert_eq!(issues[0].kind, IssueKind::EmptyGeometryForType(geom_type));
		}
	}

	#[test]
	fn detects_malformed_command_stream() {
		// Garbage bytes — unknown command code.
		let feature = VectorTileFeature {
			id: None,
			tag_ids: vec![],
			geom_type: GeomType::MultiPolygon,
			geom_data: Blob::from(vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
		};
		let tile = tile_with_layer(layer_with_features("l", vec![feature]));
		let issues = validate_tile(&tile);
		assert_eq!(issues.len(), 1);
		assert!(matches!(issues[0].kind, IssueKind::MalformedCommandStream(_)));
	}

	#[test]
	fn reports_layer_and_feature_index_correctly() {
		let good = raw_polygon_feature(&[vec![(0, 0), (4, 0), (4, 4), (0, 4)]]);
		let bad = raw_polygon_feature(&[vec![(0, 0), (0, 4), (4, 4), (4, 0)]]); // orphan inner
		let tile = tile_with_layer(layer_with_features("mixed", vec![good, bad]));
		let issues = validate_tile(&tile);
		assert_eq!(issues.len(), 1);
		assert_eq!(issues[0].layer, "mixed");
		assert_eq!(issues[0].feature_index, 1);
	}
}
