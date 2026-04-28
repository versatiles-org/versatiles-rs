//! Reassemble feature geometries from simplified arcs.
//!
//! For each [`FeatureArcs`] entry, walk the arc references in order, fetch
//! the corresponding [`Arc`] (in the right direction), and concatenate
//! coords skipping the duplicated junction vertex between consecutive arcs.

use super::{Arc, ArcRef, FeatureArcs, LineStringArcs, PolygonArcs};
use crate::geo::GeoFeature;
use geo_types::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

/// Reassemble all features from simplified arcs + per-feature ref lists.
///
/// `template` carries the original `id` and `properties` of each feature; the
/// geometry is rebuilt from `arcs` and `feature_arcs`. Both vectors must be
/// in the same order as `template`.
#[must_use]
pub fn reassemble_features(arcs: &[Arc], feature_arcs: &[FeatureArcs], template: &[GeoFeature]) -> Vec<GeoFeature> {
	feature_arcs
		.iter()
		.zip(template.iter())
		.map(|(fa, original)| GeoFeature {
			id: original.id.clone(),
			geometry: reassemble_geometry(arcs, fa),
			properties: original.properties.clone(),
		})
		.collect()
}

fn reassemble_geometry(arcs: &[Arc], fa: &FeatureArcs) -> Geometry<f64> {
	match fa {
		FeatureArcs::Point(c) => Geometry::Point(Point(*c)),
		FeatureArcs::MultiPoint(coords) => Geometry::MultiPoint(MultiPoint(coords.iter().map(|c| Point(*c)).collect())),
		FeatureArcs::LineString(la) => Geometry::LineString(reassemble_line_string(arcs, la)),
		FeatureArcs::MultiLineString(mls) => Geometry::MultiLineString(MultiLineString(
			mls.iter().map(|la| reassemble_line_string(arcs, la)).collect(),
		)),
		FeatureArcs::Polygon(pa) => Geometry::Polygon(reassemble_polygon(arcs, pa)),
		FeatureArcs::MultiPolygon(mpa) => Geometry::MultiPolygon(MultiPolygon(
			mpa.iter().map(|pa| reassemble_polygon(arcs, pa)).collect(),
		)),
	}
}

fn reassemble_line_string(arcs: &[Arc], la: &LineStringArcs) -> LineString<f64> {
	LineString::new(concat_arc_refs(arcs, &la.0))
}

fn reassemble_polygon(arcs: &[Arc], pa: &PolygonArcs) -> Polygon<f64> {
	let exterior = reassemble_ring(arcs, &pa.exterior);
	let interiors: Vec<LineString<f64>> = pa.interiors.iter().map(|r| reassemble_ring(arcs, r)).collect();
	Polygon::new(exterior, interiors)
}

fn reassemble_ring(arcs: &[Arc], refs: &[ArcRef]) -> LineString<f64> {
	let mut coords = concat_arc_refs(arcs, refs);
	// Ensure ring is closed. (For a no-junction one-arc ring the arc already
	// contains the closing duplicate; for a multi-arc ring the closure is a
	// natural consequence of the last arc ending at the start junction.)
	if !coords.is_empty() && coords.first() != coords.last() {
		let first = coords[0];
		coords.push(first);
	}
	LineString::new(coords)
}

/// Concatenate a sequence of arc references, skipping the leading vertex of
/// every arc after the first (it duplicates the previous arc's last vertex,
/// which is the junction between them).
fn concat_arc_refs(arcs: &[Arc], refs: &[ArcRef]) -> Vec<Coord<f64>> {
	let mut out: Vec<Coord<f64>> = Vec::new();
	for (i, arc_ref) in refs.iter().enumerate() {
		let arc = &arcs[arc_ref.arc_id];
		let mut iter: Box<dyn Iterator<Item = Coord<f64>>> = if arc_ref.reversed {
			Box::new(arc.coords.iter().rev().copied())
		} else {
			Box::new(arc.coords.iter().copied())
		};
		if i > 0 {
			// First coord duplicates the previous arc's last; skip it.
			let _ = iter.next();
		}
		out.extend(iter);
	}
	out
}

#[cfg(test)]
mod tests {
	use super::super::build;
	use super::*;
	use crate::geo::GeoFeature;
	use geo_types::{Geometry, LineString, Polygon};

	fn poly_feat(rings: &[Vec<[f64; 2]>]) -> GeoFeature {
		let mut iter = rings.iter().map(|r| LineString::from(r.clone()));
		let exterior = iter.next().unwrap();
		let interiors = iter.collect();
		GeoFeature::new(Geometry::Polygon(Polygon::new(exterior, interiors)))
	}

	#[test]
	fn isolated_polygon_round_trips() {
		let original = poly_feat(&[vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]]]);
		let (graph, fa) = build(std::slice::from_ref(&original)).unwrap();
		let arcs: Vec<Arc> = graph.arcs().to_vec();
		let rebuilt = reassemble_features(&arcs, &fa, std::slice::from_ref(&original));
		match (&original.geometry, &rebuilt[0].geometry) {
			(Geometry::Polygon(o), Geometry::Polygon(r)) => {
				// Same number of vertices.
				assert_eq!(o.exterior().0.len(), r.exterior().0.len());
				// Closed ring on both.
				assert_eq!(r.exterior().0.first(), r.exterior().0.last());
			}
			_ => panic!(),
		}
	}

	#[test]
	fn shared_border_simplifies_consistently() {
		// Two polygons sharing an edge with a tiny wiggle in the middle.
		// After arc-graph simplification with a tolerance that swallows the
		// wiggle, both polygons must end up with identical border coords.
		use super::super::simplify::simplify_arcs;

		let a = poly_feat(&[vec![
			[0.0, 0.0],
			[1.0, 0.0],
			[1.0, 0.5001],
			[1.0, 1.0],
			[0.0, 1.0],
			[0.0, 0.0],
		]]);
		let b = poly_feat(&[vec![
			[1.0, 0.0],
			[2.0, 0.0],
			[2.0, 1.0],
			[1.0, 1.0],
			[1.0, 0.5001],
			[1.0, 0.0],
		]]);
		let template = vec![a.clone(), b.clone()];
		let (graph, fa) = build(&template).unwrap();
		// Simplify the arcs with a tolerance large enough to drop the wiggle.
		let simplified = simplify_arcs(&graph, 0.01);
		let rebuilt = reassemble_features(&simplified, &fa, &template);

		// Compare the shared boundary coordinates between A and B by extracting
		// the vertex set on the shared line x = 1.0 from each polygon.
		fn border_at_x_eq_1(g: &Geometry<f64>) -> Vec<Coord<f64>> {
			match g {
				Geometry::Polygon(p) => p
					.exterior()
					.0
					.iter()
					.filter(|c| (c.x - 1.0).abs() < 1e-9)
					.copied()
					.collect(),
				_ => panic!(),
			}
		}
		let a_border = border_at_x_eq_1(&rebuilt[0].geometry);
		let b_border = border_at_x_eq_1(&rebuilt[1].geometry);

		// Sort by Y so direction doesn't matter for the comparison.
		let mut a_sorted = a_border.clone();
		a_sorted.sort_by(|p, q| p.y.partial_cmp(&q.y).unwrap());
		let mut b_sorted = b_border.clone();
		b_sorted.sort_by(|p, q| p.y.partial_cmp(&q.y).unwrap());
		assert_eq!(a_sorted, b_sorted, "shared border must match byte-for-byte");
	}
}
