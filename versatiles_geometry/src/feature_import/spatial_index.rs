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
