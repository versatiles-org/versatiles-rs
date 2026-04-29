//! Arc graph: a topological decomposition of polygon rings and linestrings
//! that lets shared edges be simplified once.
//!
//! Phase 5.1 builds the graph; Phase 5.2 simplifies arcs and reassembles
//! features at each zoom level.

mod extract;
mod reassemble;
mod simplify;

pub use extract::build;
pub use reassemble::{reassemble_features, reassemble_geometry};
pub use simplify::simplify_arcs;

use geo_types::Coord;
use std::collections::HashMap;

/// Stable identifier for an arc inside an [`ArcGraph`].
pub type ArcId = usize;

/// One arc — a sequence of coordinates that may be shared by multiple features.
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
	pub coords: Vec<Coord<f64>>,
}

/// A reference to an arc plus the direction in which a particular feature
/// traverses it relative to the arc's stored orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcRef {
	pub arc_id: ArcId,
	pub reversed: bool,
}

/// The graph of all distinct arcs in a feature set.
#[derive(Clone, Debug, Default)]
pub struct ArcGraph {
	pub(super) arcs: Vec<Arc>,
	/// Canonical key → arc id, used by [`extract::build`] to dedupe arcs in
	/// either direction. Not used after build; kept on the graph so the
	/// `insert` method (also private to the module) can append.
	pub(super) canonical_index: HashMap<Vec<(u64, u64)>, ArcId>,
}

impl ArcGraph {
	#[must_use]
	pub fn arcs(&self) -> &[Arc] {
		&self.arcs
	}

	#[must_use]
	pub fn arc(&self, id: ArcId) -> Option<&Arc> {
		self.arcs.get(id)
	}

	#[must_use]
	pub fn len(&self) -> usize {
		self.arcs.len()
	}

	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.arcs.is_empty()
	}
}

/// Per-feature arc-reference list. Aligns 1:1 with the input feature vec
/// passed to [`build`]. Points pass through verbatim.
#[derive(Clone, Debug)]
pub enum FeatureArcs {
	Point(Coord<f64>),
	MultiPoint(Vec<Coord<f64>>),
	LineString(LineStringArcs),
	MultiLineString(Vec<LineStringArcs>),
	Polygon(PolygonArcs),
	MultiPolygon(Vec<PolygonArcs>),
}

/// A linestring decomposed into arcs (in input order).
#[derive(Clone, Debug)]
pub struct LineStringArcs(pub(super) Vec<ArcRef>);

impl LineStringArcs {
	#[must_use]
	pub fn arcs(&self) -> &[ArcRef] {
		&self.0
	}
}

/// A polygon decomposed into arcs: one ring of arcs for the exterior, plus
/// one ring of arcs for each interior (hole).
#[derive(Clone, Debug)]
pub struct PolygonArcs {
	pub(super) exterior: Vec<ArcRef>,
	pub(super) interiors: Vec<Vec<ArcRef>>,
}

impl PolygonArcs {
	#[must_use]
	pub fn exterior(&self) -> &[ArcRef] {
		&self.exterior
	}

	#[must_use]
	pub fn interiors(&self) -> &[Vec<ArcRef>] {
		&self.interiors
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::geo::GeoFeature;
	use geo_types::{Geometry, LineString, Polygon};

	fn polygon_feature(rings: &[Vec<[f64; 2]>]) -> GeoFeature {
		let mut iter = rings.iter().map(|r| LineString::from(r.clone()));
		let exterior = iter.next().expect("at least an exterior");
		let interiors = iter.collect();
		GeoFeature::new(Geometry::Polygon(Polygon::new(exterior, interiors)))
	}

	fn line_feature(coords: Vec<[f64; 2]>) -> GeoFeature {
		GeoFeature::new(Geometry::LineString(LineString::from(coords)))
	}

	#[test]
	fn isolated_ring_becomes_one_closed_arc() {
		let p = polygon_feature(&[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]]);
		let (graph, fa) = build(&[p]);
		assert_eq!(graph.len(), 1);
		match &fa[0] {
			FeatureArcs::Polygon(pa) => {
				assert_eq!(pa.exterior.len(), 1);
				assert_eq!(pa.interiors.len(), 0);
			}
			other => panic!("expected polygon, got {other:?}"),
		}
		// Closed-arc representation: first == last.
		let arc = &graph.arcs()[0];
		assert!(arc.coords.first() == arc.coords.last());
	}

	#[test]
	fn two_polygons_share_one_arc() {
		// Two squares sharing the right/left edge.
		// A: (0,0)-(1,0)-(1,1)-(0,1)-(0,0)
		// B: (1,0)-(2,0)-(2,1)-(1,1)-(1,0)
		// Shared edge: (1,0)-(1,1)
		let a = polygon_feature(&[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]]);
		let b = polygon_feature(&[vec![[1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 0.0]]]);
		let (graph, fa) = build(&[a, b]);

		// Three arcs: A's outer (going 0,0 → 0,1 → 1,1), shared (1,1 → 1,0 or
		// reverse), B's outer (1,0 → 2,0 → 2,1 → 1,1).
		assert_eq!(graph.len(), 3);

		let FeatureArcs::Polygon(pa) = &fa[0] else { panic!() };
		let FeatureArcs::Polygon(pb) = &fa[1] else { panic!() };
		assert_eq!(pa.exterior.len(), 2);
		assert_eq!(pb.exterior.len(), 2);

		// Find the shared arc: the one that appears in both feature ref lists.
		let a_ids: std::collections::HashSet<ArcId> = pa.exterior.iter().map(|r| r.arc_id).collect();
		let b_ids: std::collections::HashSet<ArcId> = pb.exterior.iter().map(|r| r.arc_id).collect();
		let shared: Vec<ArcId> = a_ids.intersection(&b_ids).copied().collect();
		assert_eq!(shared.len(), 1, "exactly one arc is shared");

		// The two refs to the shared arc must have opposite direction flags
		// (one polygon traverses the border one way, the other the opposite).
		let a_ref = pa.exterior.iter().find(|r| r.arc_id == shared[0]).unwrap();
		let b_ref = pb.exterior.iter().find(|r| r.arc_id == shared[0]).unwrap();
		assert_ne!(a_ref.reversed, b_ref.reversed);
	}

	#[test]
	fn line_through_polygon_vertex_splits_both() {
		// A square polygon with vertices (0,0), (4,0), (4,4), (0,4).
		// A linestring passing through vertex (4,0): goes (5,-1) → (4,0) → (5,1).
		let p = polygon_feature(&[vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]]]);
		let l = line_feature(vec![[5.0, -1.0], [4.0, 0.0], [5.0, 1.0]]);
		let (graph, fa) = build(&[p, l]);

		// Polygon should split at (4,0). Plus the line splits at (4,0). Plus
		// the line endpoints are junctions (open-line endpoints), but the line
		// has only 3 vertices so endpoint split = no extra split.
		// Polygon arcs: 1 (the ring rotates to start at (4,0), no other junctions).
		// Line arcs: 2 ((5,-1)→(4,0) and (4,0)→(5,1)).
		// Total: 3 distinct arcs.
		assert_eq!(graph.len(), 3);

		match &fa[0] {
			FeatureArcs::Polygon(pa) => assert_eq!(pa.exterior.len(), 1),
			_ => panic!(),
		}
		match &fa[1] {
			FeatureArcs::LineString(la) => assert_eq!(la.0.len(), 2),
			_ => panic!(),
		}
	}

	#[test]
	fn three_way_junction_splits_lines() {
		// Two lines that meet at a single shared interior vertex (1,0):
		//   line A: (0,0) → (1,0) → (2,0)
		//   line B: (1,-1) → (1,0) → (1,1)
		// (1,0) sees neighbors {(0,0), (2,0), (1,-1), (1,1)} — size 4 → junction.
		let a = line_feature(vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]]);
		let b = line_feature(vec![[1.0, -1.0], [1.0, 0.0], [1.0, 1.0]]);
		let (graph, fa) = build(&[a, b]);

		// 4 arcs: A splits into 2, B splits into 2. None are shared (different
		// neighbors), so total = 4 distinct arcs.
		assert_eq!(graph.len(), 4);
		match &fa[0] {
			FeatureArcs::LineString(la) => assert_eq!(la.0.len(), 2),
			_ => panic!(),
		}
		match &fa[1] {
			FeatureArcs::LineString(lb) => assert_eq!(lb.0.len(), 2),
			_ => panic!(),
		}
	}

	#[test]
	fn duplicate_isolated_rings_dedupe() {
		// Two identical isolated polygons — one arc shared between both.
		let a = polygon_feature(&[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]]);
		let b = polygon_feature(&[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]]);
		let (graph, fa) = build(&[a, b]);
		// Hmm, both polygons "share" the entire ring as common. Every vertex
		// has the same {prev, next} neighbor set in both polygons → no
		// junctions. So both rings produce one closed arc each, and they
		// collapse into a single canonical arc via dedupe.
		assert_eq!(graph.len(), 1);
		let FeatureArcs::Polygon(pa) = &fa[0] else { panic!() };
		let FeatureArcs::Polygon(pb) = &fa[1] else { panic!() };
		assert_eq!(pa.exterior.len(), 1);
		assert_eq!(pb.exterior.len(), 1);
		assert_eq!(pa.exterior[0].arc_id, pb.exterior[0].arc_id);
	}

	#[test]
	fn point_passes_through() {
		let p = GeoFeature::new(Geometry::Point(geo_types::Point::new(13.4, 52.5)));
		let (graph, fa) = build(&[p]);
		assert_eq!(graph.len(), 0);
		match &fa[0] {
			FeatureArcs::Point(c) => {
				assert!((c.x - 13.4).abs() < 1e-12);
				assert!((c.y - 52.5).abs() < 1e-12);
			}
			_ => panic!(),
		}
	}
}
