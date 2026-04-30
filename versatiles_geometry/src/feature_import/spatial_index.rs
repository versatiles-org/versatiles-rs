//! Spatial index of feature bounding boxes for one zoom level.

use rstar::{AABB, RTree, RTreeObject};

/// A reference to a feature in a [`super::ZoomLayer`]'s feature list, indexed
/// by an `AABB` over the feature's mercator bounding box.
#[derive(Clone, Copy, Debug)]
pub struct FeatureRef {
	pub index: usize,
	envelope: AABB<[f64; 2]>,
}

impl FeatureRef {
	#[must_use]
	pub fn new(index: usize, bbox: [f64; 4]) -> Self {
		Self {
			index,
			envelope: AABB::from_corners([bbox[0], bbox[1]], [bbox[2], bbox[3]]),
		}
	}
}

impl RTreeObject for FeatureRef {
	type Envelope = AABB<[f64; 2]>;

	fn envelope(&self) -> Self::Envelope {
		self.envelope
	}
}

/// Returns an iterator over feature refs whose envelope intersects `bbox`.
pub fn query(rtree: &RTree<FeatureRef>, bbox: [f64; 4]) -> impl Iterator<Item = &FeatureRef> {
	let envelope = AABB::from_corners([bbox[0], bbox[1]], [bbox[2], bbox[3]]);
	rtree.locate_in_envelope_intersecting(&envelope)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn rtree_with(refs: Vec<FeatureRef>) -> RTree<FeatureRef> {
		RTree::bulk_load(refs)
	}

	#[test]
	fn query_returns_intersecting_features() {
		let tree = rtree_with(vec![
			FeatureRef::new(0, [0.0, 0.0, 1.0, 1.0]),
			FeatureRef::new(1, [10.0, 10.0, 11.0, 11.0]),
			FeatureRef::new(2, [0.5, 0.5, 5.0, 5.0]),
		]);
		let mut hits: Vec<usize> = query(&tree, [0.0, 0.0, 2.0, 2.0]).map(|r| r.index).collect();
		hits.sort_unstable();
		assert_eq!(hits, vec![0, 2]);
	}

	#[test]
	fn query_outside_yields_nothing() {
		let tree = rtree_with(vec![FeatureRef::new(0, [0.0, 0.0, 1.0, 1.0])]);
		let hits: Vec<_> = query(&tree, [10.0, 10.0, 20.0, 20.0]).collect();
		assert!(hits.is_empty());
	}

	#[test]
	fn query_touching_edge_intersects() {
		// AABB intersection is inclusive — boxes touching at an edge count.
		let tree = rtree_with(vec![FeatureRef::new(7, [0.0, 0.0, 1.0, 1.0])]);
		let hits: Vec<usize> = query(&tree, [1.0, 0.0, 2.0, 1.0]).map(|r| r.index).collect();
		assert_eq!(hits, vec![7]);
	}

	#[test]
	fn empty_tree_query_yields_nothing() {
		let tree: RTree<FeatureRef> = rtree_with(vec![]);
		assert_eq!(query(&tree, [0.0, 0.0, 1.0, 1.0]).count(), 0);
	}
}
