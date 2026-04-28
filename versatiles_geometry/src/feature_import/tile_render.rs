//! Per-tile rendering: clip features to the tile bbox, quantize to the
//! tile-local 4096×4096 grid, and encode as MVT.
//!
//! Polygon clipping uses Sutherland-Hodgman per ring; line clipping uses
//! Liang-Barsky per segment. Both are textbook algorithms — they're written
//! by hand here to avoid pulling in heavier polygon-boolean machinery for
//! Phase 1.

use crate::geo::GeoFeature;
use crate::vector_tile::{VectorTile, VectorTileLayer};
use anyhow::Result;
use geo::MapCoords;
use geo_types::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Polygon};

const MVT_VERSION: u32 = 1;

/// Clip every feature to `tile_bbox` (mercator), quantize to the tile-local
/// `[0, extent]` grid, encode as a single-layer MVT. Returns `Ok(None)` if
/// no feature survives clipping.
pub fn render_tile(
	features: impl IntoIterator<Item = GeoFeature>,
	layer_name: &str,
	tile_bbox: [f64; 4],
	extent: u32,
) -> Result<Option<VectorTile>> {
	let mut clipped: Vec<GeoFeature> = Vec::new();
	for feature in features {
		let GeoFeature {
			id,
			geometry,
			properties,
		} = feature;
		for piece in clip_geometry(geometry, tile_bbox) {
			let quantized = quantize_geometry(&piece, tile_bbox, extent);
			clipped.push(GeoFeature {
				id: id.clone(),
				geometry: quantized,
				properties: properties.clone(),
			});
		}
	}

	if clipped.is_empty() {
		return Ok(None);
	}

	let layer = VectorTileLayer::from_features(layer_name.to_string(), clipped, extent, MVT_VERSION)?;
	Ok(Some(VectorTile::new(vec![layer])))
}

/// Clip a single geometry to `bbox`. May produce zero, one, or multiple
/// output geometries (e.g., a polyline that exits and re-enters the tile
/// becomes a `MultiLineString`).
pub fn clip_geometry(g: Geometry<f64>, bbox: [f64; 4]) -> Vec<Geometry<f64>> {
	let [xmin, ymin, xmax, ymax] = bbox;
	let in_bbox = |c: Coord<f64>| c.x >= xmin && c.x <= xmax && c.y >= ymin && c.y <= ymax;

	match g {
		Geometry::Point(p) => {
			if in_bbox(p.0) {
				vec![Geometry::Point(p)]
			} else {
				Vec::new()
			}
		}
		Geometry::MultiPoint(mp) => {
			let pts: Vec<_> = mp.0.into_iter().filter(|p| in_bbox(p.0)).collect();
			if pts.is_empty() {
				Vec::new()
			} else {
				vec![Geometry::MultiPoint(MultiPoint(pts))]
			}
		}
		Geometry::LineString(ls) => {
			let parts = clip_line_string(&ls, bbox);
			match parts.len() {
				0 => Vec::new(),
				1 => vec![Geometry::LineString(parts.into_iter().next().expect("len == 1"))],
				_ => vec![Geometry::MultiLineString(MultiLineString(parts))],
			}
		}
		Geometry::MultiLineString(ml) => {
			let mut all: Vec<LineString<f64>> = Vec::new();
			for ls in ml.0 {
				all.extend(clip_line_string(&ls, bbox));
			}
			if all.is_empty() {
				Vec::new()
			} else {
				vec![Geometry::MultiLineString(MultiLineString(all))]
			}
		}
		Geometry::Polygon(p) => clip_polygon(&p, bbox).into_iter().map(Geometry::Polygon).collect(),
		Geometry::MultiPolygon(mp) => {
			let mut all: Vec<Polygon<f64>> = Vec::new();
			for p in mp.0 {
				all.extend(clip_polygon(&p, bbox));
			}
			if all.is_empty() {
				Vec::new()
			} else {
				vec![Geometry::MultiPolygon(MultiPolygon(all))]
			}
		}
		_ => Vec::new(),
	}
}

fn quantize_geometry(g: &Geometry<f64>, tile_bbox: [f64; 4], extent: u32) -> Geometry<f64> {
	let [xmin, _, xmax, ymax] = tile_bbox;
	let scale = f64::from(extent) / (xmax - xmin);
	g.map_coords(|c| Coord {
		x: (c.x - xmin) * scale,
		y: (ymax - c.y) * scale, // tile-local Y is flipped (top-left origin)
	})
}

// ─── Line clipping ────────────────────────────────────────────────────────

/// Liang-Barsky clipper for a single segment. Returns `None` if the segment
/// lies entirely outside `bbox`, else the clipped endpoints.
///
/// The single-letter binding names (`a`, `b`, `p`, `q`, `t`) follow the
/// canonical Liang-Barsky paper notation; renaming would only obscure them.
#[allow(clippy::many_single_char_names)]
fn clip_segment(a: Coord<f64>, b: Coord<f64>, bbox: [f64; 4]) -> Option<(Coord<f64>, Coord<f64>)> {
	let [xmin, ymin, xmax, ymax] = bbox;
	let dx = b.x - a.x;
	let dy = b.y - a.y;
	let p = [-dx, dx, -dy, dy];
	let q = [a.x - xmin, xmax - a.x, a.y - ymin, ymax - a.y];
	let mut t0 = 0.0_f64;
	let mut t1 = 1.0_f64;
	for i in 0..4 {
		if p[i] == 0.0 {
			if q[i] < 0.0 {
				return None;
			}
		} else {
			let t = q[i] / p[i];
			if p[i] < 0.0 {
				if t > t1 {
					return None;
				}
				if t > t0 {
					t0 = t;
				}
			} else {
				if t < t0 {
					return None;
				}
				if t < t1 {
					t1 = t;
				}
			}
		}
	}
	Some((
		Coord {
			x: a.x + t0 * dx,
			y: a.y + t0 * dy,
		},
		Coord {
			x: a.x + t1 * dx,
			y: a.y + t1 * dy,
		},
	))
}

/// Clip a polyline. May return multiple disjoint pieces if it exits and re-enters `bbox`.
fn clip_line_string(ls: &LineString<f64>, bbox: [f64; 4]) -> Vec<LineString<f64>> {
	let mut result: Vec<LineString<f64>> = Vec::new();
	let mut current: Vec<Coord<f64>> = Vec::new();

	for win in ls.0.windows(2) {
		let a = win[0];
		let b = win[1];
		match clip_segment(a, b, bbox) {
			Some((ca, cb)) => {
				if current.is_empty() || current.last().copied() != Some(ca) {
					if current.len() >= 2 {
						result.push(LineString::new(std::mem::take(&mut current)));
					} else {
						current.clear();
					}
					current.push(ca);
				}
				current.push(cb);
			}
			None => {
				if current.len() >= 2 {
					result.push(LineString::new(std::mem::take(&mut current)));
				} else {
					current.clear();
				}
			}
		}
	}
	if current.len() >= 2 {
		result.push(LineString::new(current));
	}
	result
}

// ─── Polygon clipping (Sutherland-Hodgman) ────────────────────────────────

fn intersect_x(a: Coord<f64>, b: Coord<f64>, x: f64) -> Coord<f64> {
	let t = (x - a.x) / (b.x - a.x);
	Coord {
		x,
		y: a.y + t * (b.y - a.y),
	}
}

fn intersect_y(a: Coord<f64>, b: Coord<f64>, y: f64) -> Coord<f64> {
	let t = (y - a.y) / (b.y - a.y);
	Coord {
		x: a.x + t * (b.x - a.x),
		y,
	}
}

fn sh_edge(
	input: &[Coord<f64>],
	inside: impl Fn(Coord<f64>) -> bool,
	intersect: impl Fn(Coord<f64>, Coord<f64>) -> Coord<f64>,
) -> Vec<Coord<f64>> {
	if input.is_empty() {
		return Vec::new();
	}
	// Treat the ring as open: drop the duplicate closing vertex if present.
	let len = if input.len() >= 2 && input.first() == input.last() {
		input.len() - 1
	} else {
		input.len()
	};
	if len == 0 {
		return Vec::new();
	}
	let mut out = Vec::with_capacity(len);
	let mut prev = input[len - 1];
	let mut prev_in = inside(prev);
	for &curr in &input[..len] {
		let curr_in = inside(curr);
		if curr_in {
			if !prev_in {
				out.push(intersect(prev, curr));
			}
			out.push(curr);
		} else if prev_in {
			out.push(intersect(prev, curr));
		}
		prev = curr;
		prev_in = curr_in;
	}
	out
}

fn clip_ring(ring: &LineString<f64>, bbox: [f64; 4]) -> Option<LineString<f64>> {
	let [xmin, ymin, xmax, ymax] = bbox;
	let mut v: Vec<Coord<f64>> = ring.0.clone();
	v = sh_edge(&v, |c| c.x >= xmin, |a, b| intersect_x(a, b, xmin));
	if v.is_empty() {
		return None;
	}
	v = sh_edge(&v, |c| c.x <= xmax, |a, b| intersect_x(a, b, xmax));
	if v.is_empty() {
		return None;
	}
	v = sh_edge(&v, |c| c.y >= ymin, |a, b| intersect_y(a, b, ymin));
	if v.is_empty() {
		return None;
	}
	v = sh_edge(&v, |c| c.y <= ymax, |a, b| intersect_y(a, b, ymax));
	if v.len() < 3 {
		return None;
	}
	// Re-close the ring.
	v.push(v[0]);
	Some(LineString::new(v))
}

fn clip_polygon(p: &Polygon<f64>, bbox: [f64; 4]) -> Vec<Polygon<f64>> {
	let Some(exterior) = clip_ring(p.exterior(), bbox) else {
		return Vec::new();
	};
	let interiors: Vec<_> = p.interiors().iter().filter_map(|r| clip_ring(r, bbox)).collect();
	vec![Polygon::new(exterior, interiors)]
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{LineString, Point, Polygon};

	#[test]
	fn point_inside_kept() {
		let g = Geometry::Point(Point::new(0.5, 0.5));
		assert_eq!(clip_geometry(g, [0.0, 0.0, 1.0, 1.0]).len(), 1);
	}

	#[test]
	fn point_outside_dropped() {
		let g = Geometry::Point(Point::new(2.0, 2.0));
		assert!(clip_geometry(g, [0.0, 0.0, 1.0, 1.0]).is_empty());
	}

	#[test]
	fn line_clipped_to_one_piece() {
		let ls = LineString::from(vec![[-1.0, 0.5], [2.0, 0.5]]);
		let out = clip_geometry(Geometry::LineString(ls), [0.0, 0.0, 1.0, 1.0]);
		assert_eq!(out.len(), 1);
		match &out[0] {
			Geometry::LineString(ls) => {
				assert_eq!(ls.0.len(), 2);
				assert!((ls.0[0].x - 0.0).abs() < 1e-9);
				assert!((ls.0[1].x - 1.0).abs() < 1e-9);
			}
			other => panic!("expected LineString, got {other:?}"),
		}
	}

	#[test]
	fn line_split_into_multi() {
		// Polyline visits the bbox, escapes far enough that an entire segment
		// is outside (None from Liang-Barsky), then re-enters. Result: two pieces.
		let ls = LineString::from(vec![[0.5, 0.5], [3.0, 3.0], [4.0, 3.0], [0.5, 0.5]]);
		let out = clip_geometry(Geometry::LineString(ls), [0.0, 0.0, 1.0, 1.0]);
		assert_eq!(out.len(), 1);
		match &out[0] {
			Geometry::MultiLineString(ml) => assert_eq!(ml.0.len(), 2),
			other => panic!("expected MultiLineString, got {other:?}"),
		}
	}

	#[test]
	fn polygon_partial_overlap_clipped() {
		// A 2×2 square spanning [-0.5..1.5] in x and [-0.5..1.5] in y, clipped to [0..1].
		let exterior = LineString::from(vec![[-0.5, -0.5], [1.5, -0.5], [1.5, 1.5], [-0.5, 1.5], [-0.5, -0.5]]);
		let p = Polygon::new(exterior, vec![]);
		let out = clip_geometry(Geometry::Polygon(p), [0.0, 0.0, 1.0, 1.0]);
		assert_eq!(out.len(), 1);
		match &out[0] {
			Geometry::Polygon(p) => {
				assert_eq!(p.exterior().0.len(), 5); // closed quad
				for c in &p.exterior().0 {
					assert!(c.x >= 0.0 && c.x <= 1.0 && c.y >= 0.0 && c.y <= 1.0);
				}
			}
			other => panic!("expected Polygon, got {other:?}"),
		}
	}

	#[test]
	fn quantize_maps_corners() {
		let g = Geometry::Point(Point::new(0.25, 0.75));
		// tile bbox: [0..1, 0..1]; extent: 4096
		let q = quantize_geometry(&g, [0.0, 0.0, 1.0, 1.0], 4096);
		match q {
			Geometry::Point(p) => {
				assert!((p.x() - 1024.0).abs() < 1e-6);
				// Y is flipped: y_in = 0.75 → y_out = (1.0 - 0.75) * 4096 = 1024
				assert!((p.y() - 1024.0).abs() < 1e-6);
			}
			_ => panic!("expected Point"),
		}
	}
}
