//! Arc-graph extraction from a flat feature list.
//!
//! Given a set of features, build an arc graph where every shared ring
//! segment or shared line segment is represented exactly once. Each feature
//! is replaced by a list of [`ArcRef`]s pointing into the graph, with a
//! direction flag indicating whether the feature traverses the arc forwards
//! or backwards relative to the graph's stored orientation.
//!
//! # Junction detection
//!
//! A vertex is a *junction* — i.e. an arc endpoint — iff:
//!
//! - Its set of distinct neighboring vertex positions across all features
//!   has size other than 2, OR
//! - It is an endpoint of an open linestring.
//!
//! With that definition, the *interior* of a shared border between two
//! polygons is never a junction (interior vertex sees the same two neighbors
//! in both polygons), but the endpoints of the shared border are
//! (each sees three: the shared neighbor, plus each polygon's diverging
//! non-shared neighbor).

use super::{Arc, ArcGraph, ArcId, ArcRef, FeatureArcs, LineStringArcs, PolygonArcs};
use crate::geo::GeoFeature;
use anyhow::Result;
use geo_types::{Coord, Geometry, LineString, Polygon};
use std::collections::{HashMap, HashSet};

/// Bit-exact, direction-comparable key for a coordinate.
type CoordKey = (u64, u64);

#[inline]
fn key(c: Coord<f64>) -> CoordKey {
	(c.x.to_bits(), c.y.to_bits())
}

/// Build an arc graph from `features` and return the per-feature arc-reference list.
/// The returned `Vec<FeatureArcs>` aligns 1:1 with `features`.
pub fn build(features: &[GeoFeature]) -> Result<(ArcGraph, Vec<FeatureArcs>)> {
	// Pass 1: gather neighbor sets to identify junctions.
	let junctions = identify_junctions(features);

	// Pass 2: split each ring/line at junctions, dedupe arcs, build references.
	let mut graph = ArcGraph::default();
	let mut feature_arcs: Vec<FeatureArcs> = Vec::with_capacity(features.len());
	for feature in features {
		let entry = match &feature.geometry {
			Geometry::Point(p) => FeatureArcs::Point(p.0),
			Geometry::MultiPoint(mp) => FeatureArcs::MultiPoint(mp.0.iter().map(|p| p.0).collect()),
			Geometry::LineString(ls) => FeatureArcs::LineString(linestring_to_arcs(ls, &junctions, &mut graph)),
			Geometry::MultiLineString(ml) => FeatureArcs::MultiLineString(
				ml.0
					.iter()
					.map(|ls| linestring_to_arcs(ls, &junctions, &mut graph))
					.collect(),
			),
			Geometry::Polygon(p) => FeatureArcs::Polygon(polygon_to_arcs(p, &junctions, &mut graph)),
			Geometry::MultiPolygon(mp) => FeatureArcs::MultiPolygon(
				mp.0
					.iter()
					.map(|p| polygon_to_arcs(p, &junctions, &mut graph))
					.collect(),
			),
			// Other variants (Line, Rect, Triangle, GeometryCollection) aren't
			// produced by the import pipeline; treat as empty.
			_ => FeatureArcs::Point(Coord { x: 0.0, y: 0.0 }),
		};
		feature_arcs.push(entry);
	}
	Ok((graph, feature_arcs))
}

/// Walk every ring and line, collect each vertex's distinct neighbor set, and
/// return the junction set (vertices with neighbor-count != 2, plus all
/// open-line endpoints).
fn identify_junctions(features: &[GeoFeature]) -> HashSet<CoordKey> {
	let mut neighbors: HashMap<CoordKey, HashSet<CoordKey>> = HashMap::new();
	let mut line_endpoints: HashSet<CoordKey> = HashSet::new();

	for feature in features {
		match &feature.geometry {
			Geometry::LineString(ls) => add_line(ls, &mut neighbors, &mut line_endpoints),
			Geometry::MultiLineString(ml) => {
				for ls in &ml.0 {
					add_line(ls, &mut neighbors, &mut line_endpoints);
				}
			}
			Geometry::Polygon(p) => add_polygon(p, &mut neighbors),
			Geometry::MultiPolygon(mp) => {
				for p in &mp.0 {
					add_polygon(p, &mut neighbors);
				}
			}
			_ => {}
		}
	}

	let mut junctions = line_endpoints;
	for (vertex, n_set) in &neighbors {
		if n_set.len() != 2 {
			junctions.insert(*vertex);
		}
	}
	junctions
}

fn add_polygon(p: &Polygon<f64>, neighbors: &mut HashMap<CoordKey, HashSet<CoordKey>>) {
	add_ring(p.exterior(), neighbors);
	for interior in p.interiors() {
		add_ring(interior, neighbors);
	}
}

fn add_ring(ring: &LineString<f64>, neighbors: &mut HashMap<CoordKey, HashSet<CoordKey>>) {
	let coords = effective_ring(ring);
	let n = coords.len();
	if n < 2 {
		return;
	}
	for i in 0..n {
		let prev = key(coords[(i + n - 1) % n]);
		let curr = key(coords[i]);
		let next = key(coords[(i + 1) % n]);
		let entry = neighbors.entry(curr).or_default();
		entry.insert(prev);
		entry.insert(next);
	}
}

fn add_line(
	line: &LineString<f64>,
	neighbors: &mut HashMap<CoordKey, HashSet<CoordKey>>,
	endpoints: &mut HashSet<CoordKey>,
) {
	let coords = &line.0;
	if coords.len() < 2 {
		return;
	}
	for i in 0..coords.len() {
		let curr = key(coords[i]);
		let entry = neighbors.entry(curr).or_default();
		if i > 0 {
			entry.insert(key(coords[i - 1]));
		}
		if i < coords.len() - 1 {
			entry.insert(key(coords[i + 1]));
		}
	}
	endpoints.insert(key(coords[0]));
	endpoints.insert(key(coords[coords.len() - 1]));
}

/// Drop the closing duplicate from a closed ring (if present) so all consumers
/// see a "raw" sequence of unique vertices.
fn effective_ring(ring: &LineString<f64>) -> Vec<Coord<f64>> {
	let coords = &ring.0;
	if coords.len() >= 2 && coords.first() == coords.last() {
		coords[..coords.len() - 1].to_vec()
	} else {
		coords.clone()
	}
}

fn polygon_to_arcs(p: &Polygon<f64>, junctions: &HashSet<CoordKey>, graph: &mut ArcGraph) -> PolygonArcs {
	PolygonArcs {
		exterior: ring_to_arcs(p.exterior(), junctions, graph),
		interiors: p
			.interiors()
			.iter()
			.map(|r| ring_to_arcs(r, junctions, graph))
			.collect(),
	}
}

fn ring_to_arcs(ring: &LineString<f64>, junctions: &HashSet<CoordKey>, graph: &mut ArcGraph) -> Vec<ArcRef> {
	let coords = effective_ring(ring);
	if coords.len() < 2 {
		return Vec::new();
	}

	// Find a junction to start at; if none, the ring is a single closed arc.
	let start = (0..coords.len()).find(|&i| junctions.contains(&key(coords[i])));
	let Some(start) = start else {
		// One closed arc. Append the closing vertex so reassembly knows it's a loop.
		let mut arc_coords = coords.clone();
		arc_coords.push(arc_coords[0]);
		return vec![graph.insert(arc_coords)];
	};

	// Rotate so we start at a junction; append the starting junction at the
	// end so the last arc terminates correctly.
	let n = coords.len();
	let mut walk: Vec<Coord<f64>> = (0..n).map(|i| coords[(start + i) % n]).collect();
	walk.push(walk[0]);

	split_walk(&walk, junctions, graph)
}

fn linestring_to_arcs(ls: &LineString<f64>, junctions: &HashSet<CoordKey>, graph: &mut ArcGraph) -> LineStringArcs {
	if ls.0.len() < 2 {
		return LineStringArcs(Vec::new());
	}
	LineStringArcs(split_walk(&ls.0, junctions, graph))
}

/// Split a linear sequence into arcs at every interior junction vertex.
fn split_walk(walk: &[Coord<f64>], junctions: &HashSet<CoordKey>, graph: &mut ArcGraph) -> Vec<ArcRef> {
	let mut refs: Vec<ArcRef> = Vec::new();
	if walk.len() < 2 {
		return refs;
	}
	let mut current: Vec<Coord<f64>> = vec![walk[0]];
	for i in 1..walk.len() {
		current.push(walk[i]);
		// Only split *interior* junctions: don't split right before the last vertex.
		if i < walk.len() - 1 && junctions.contains(&key(walk[i])) {
			let next = vec![walk[i]];
			let arc_coords = std::mem::replace(&mut current, next);
			refs.push(graph.insert(arc_coords));
		}
	}
	if current.len() >= 2 {
		refs.push(graph.insert(current));
	}
	refs
}

impl ArcGraph {
	/// Insert `coords` as an arc, deduplicating against any existing arc that
	/// holds the same coordinate sequence (in either direction). Returns an
	/// [`ArcRef`] that points into the graph with the correct direction flag.
	pub fn insert(&mut self, coords: Vec<Coord<f64>>) -> ArcRef {
		let forward: Vec<CoordKey> = coords.iter().map(|c| key(*c)).collect();
		let reverse: Vec<CoordKey> = forward.iter().rev().copied().collect();
		let input_is_canonical = forward <= reverse;
		let canonical_keys = if input_is_canonical { forward } else { reverse };

		if let Some(&arc_id) = self.lookup_canonical(&canonical_keys) {
			return ArcRef {
				arc_id,
				reversed: !input_is_canonical,
			};
		}
		let arc_id: ArcId = self.arcs.len();
		let canonical_coords: Vec<Coord<f64>> = if input_is_canonical {
			coords
		} else {
			coords.into_iter().rev().collect()
		};
		self.arcs.push(Arc {
			coords: canonical_coords,
		});
		self.canonical_index.insert(canonical_keys, arc_id);
		ArcRef {
			arc_id,
			reversed: !input_is_canonical,
		}
	}

	fn lookup_canonical(&self, keys: &[CoordKey]) -> Option<&ArcId> {
		self.canonical_index.get(keys)
	}
}
