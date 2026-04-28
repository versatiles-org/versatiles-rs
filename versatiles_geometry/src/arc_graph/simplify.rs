//! Per-zoom Douglas-Peucker simplification of arc-graph arcs.
//!
//! Simplifying each arc once and then reassembling shared boundaries from
//! the same simplified arc is what guarantees neighbouring polygons stay
//! aligned: two polygons that share an arc now share the *same* simplified
//! coordinate sequence, byte-for-byte.

use super::{Arc, ArcGraph};
use geo::Simplify;
use geo_types::LineString;

/// Returns a `Vec<Arc>` aligned with `graph.arcs()` where every arc has been
/// simplified at `tolerance` (mercator-meters). Endpoints are preserved.
///
/// `tolerance <= 0.0` returns the original arcs unchanged (fast path: clone).
#[must_use]
pub fn simplify_arcs(graph: &ArcGraph, tolerance: f64) -> Vec<Arc> {
	if tolerance <= 0.0 {
		return graph.arcs().to_vec();
	}
	graph.arcs().iter().map(|arc| simplify_arc(arc, tolerance)).collect()
}

fn simplify_arc(arc: &Arc, tolerance: f64) -> Arc {
	if arc.coords.len() < 3 {
		// Fewer than 3 points: nothing for DP to drop.
		return arc.clone();
	}
	let ls = LineString::from(arc.coords.clone());
	let simplified = ls.simplify(tolerance);
	Arc { coords: simplified.0 }
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::Coord;

	fn arc(coords: Vec<[f64; 2]>) -> Arc {
		Arc {
			coords: coords.into_iter().map(|[x, y]| Coord { x, y }).collect(),
		}
	}

	#[test]
	fn preserves_endpoints() {
		// A nearly-straight 5-point arc; everything in the middle is below
		// tolerance — DP keeps just the endpoints.
		let a = arc(vec![[0.0, 0.0], [1.0, 0.001], [2.0, -0.001], [3.0, 0.0005], [4.0, 0.0]]);
		let s = simplify_arc(&a, 0.5);
		assert_eq!(s.coords.first(), a.coords.first());
		assert_eq!(s.coords.last(), a.coords.last());
		assert!(s.coords.len() < a.coords.len());
	}

	#[test]
	fn tolerance_zero_is_noop() {
		let a = arc(vec![[0.0, 0.0], [1.0, 0.5], [2.0, 0.0]]);
		let s = simplify_arc(&a, 0.0);
		assert_eq!(s.coords, a.coords);
	}

	#[test]
	fn shared_arc_is_simplified_once() {
		// Two polygons that share a wiggly border get the *same* simplified
		// arc — that's the whole point.
		let mut graph = ArcGraph::default();
		let _ = graph.insert(vec![
			Coord { x: 0.0, y: 0.0 },
			Coord { x: 1.0, y: 0.001 },
			Coord { x: 2.0, y: -0.002 },
			Coord { x: 3.0, y: 0.0 },
		]);
		let simplified = simplify_arcs(&graph, 0.5);
		assert_eq!(simplified.len(), 1);
		// Both polygons would now look up arc id 0 and get the same simplified coords.
		assert_eq!(simplified[0].coords.first(), Some(&Coord { x: 0.0, y: 0.0 }));
		assert_eq!(simplified[0].coords.last(), Some(&Coord { x: 3.0, y: 0.0 }));
	}
}
