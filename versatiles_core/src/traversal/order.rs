//! Utilities for tile traversal ordering.
//!
//! Provides the `TraversalOrder` enum and associated functions for sorting
//! tile bounding boxes according to different traversal strategies:
//! depth-first quadtree and Hilbert curve.

use crate::{TileBBox, utils::HilbertIndex};
use anyhow::{Result, bail};
use enumset::EnumSetType;

/// Strategies for ordering tiles when traversing a tile pyramid.
///
/// - `AnyOrder`: no specific ordering; leaves tiles in input order.
/// - `DepthFirst`: quadtree depth-first ordering based on x/y bits.
/// - `PMTiles`: ordering by Hilbert curve index (`PMTiles` style).
#[derive(EnumSetType)]
pub enum TraversalOrder {
	AnyOrder,
	DepthFirst,
	PMTiles,
}

impl TraversalOrder {
	/// Sorts the given slice of `TileBBox` in-place using this traversal order.
	///
	/// * `bboxes` – mutable slice of tile bounding boxes to sort.
	/// * `size` – block size used to compute quadtree coordinates for `DepthFirst`.
	pub fn sort_bboxes(&self, bboxes: &mut Vec<TileBBox>, size: u32) {
		use TraversalOrder::*;
		match self {
			AnyOrder => {}
			DepthFirst => sort_depth_first(bboxes, size),
			PMTiles => sort_hilbert(bboxes),
		}
	}

	#[must_use]
	pub fn verify_order(&self, bboxes: &[TileBBox], size: u32) -> bool {
		use TraversalOrder::*;
		let mut clone = bboxes.to_vec();
		match self {
			AnyOrder => return true,
			DepthFirst => sort_depth_first(&mut clone, size),
			PMTiles => sort_hilbert(&mut clone),
		}
		clone == bboxes
	}

	/// Merge another `TraversalOrder` into this one, choosing a compatible order.
	///
	/// If either is `AnyOrder`, results in the other order.
	/// Returns an error if both orders are concrete and different.
	pub fn intersect(&mut self, other: &TraversalOrder) -> Result<()> {
		use TraversalOrder::*;
		if self == other || other == &AnyOrder {
			return Ok(());
		}
		if self == &AnyOrder {
			*self = *other;
			return Ok(());
		}
		bail!("Incompatible traversal orders, cannot merge {self:?} with {other:?}");
	}

	pub fn get_intersected(&self, other: &TraversalOrder) -> Result<TraversalOrder> {
		let mut result = *self;
		result.intersect(other)?;
		Ok(result)
	}
}

impl std::fmt::Debug for TraversalOrder {
	/// Format the traversal order as a string.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let name = match self {
			TraversalOrder::AnyOrder => "AnyOrder",
			TraversalOrder::DepthFirst => "DepthFirst",
			TraversalOrder::PMTiles => "PMTiles",
		};
		write!(f, "{name}")
	}
}

/// In-place depth-first (quadtree) sort of tile bounding boxes.
///
/// Constructs a quadtree key by interleaving x/y bits (MSB first) and
/// sorts boxes lexicographically by this key plus a sentinel.
///
/// * `bboxes` – slice of boxes to sort.
/// * `size` – block dimension for computing tile indices.
fn sort_depth_first(bboxes: &mut Vec<TileBBox>, size: u32) {
	// Remove empty boxes
	bboxes.retain(|b| !b.is_empty());

	// Sort by quadtree path key
	bboxes.sort_by_cached_key(|b| {
		// Build a depth-first key: quadtree path (MSB first) plus sentinel 4
		let mut k = Vec::with_capacity(b.level as usize + 1);
		for i in (0..b.level).rev() {
			let bit_x = (((b.x_min().unwrap() / size) >> i) & 1) as u8;
			let bit_y = (((b.y_min().unwrap() / size) >> i) & 1) as u8;
			k.push(bit_x | (bit_y << 1));
		}
		k.push(4);
		k
	});
}

/// In-place Hilbert curve sort of tile bounding boxes.
///
/// Uses each box’s `get_hilbert_index()` as the sort key.
///
/// * `bboxes` – slice of boxes to sort.
fn sort_hilbert(bboxes: &mut [TileBBox]) {
	bboxes.sort_by_cached_key(|b| b.get_hilbert_index().unwrap());
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Build a `TileBBox` at given level, x, y.
	fn make_bbox(level: u8, x: u32, y: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x, y, x, y).unwrap()
	}

	#[test]
	fn test_sort_bboxes_any_order() {
		let mut bboxes = vec![make_bbox(1, 1, 1), make_bbox(0, 0, 0), make_bbox(1, 0, 1)];
		let original = bboxes.clone();
		TraversalOrder::AnyOrder.sort_bboxes(&mut bboxes, 1);
		assert_eq!(bboxes, original, "AnyOrder should not change order");
	}

	#[test]
	fn test_sort_bboxes_depth_first() {
		// Expect quadtree key order: level 0 then depth-first of level 1...
		let mut bboxes = vec![
			make_bbox(1, 1, 0), // key [1,0]
			make_bbox(0, 0, 0), // key []
			make_bbox(1, 0, 1), // key [2,1]
		];
		let mut expected = bboxes.clone();
		sort_depth_first(&mut expected, 1);
		TraversalOrder::DepthFirst.sort_bboxes(&mut bboxes, 1);
		assert_eq!(bboxes, expected, "DepthFirst sort matches direct sort_depth_first");
	}

	#[test]
	fn test_sort_bboxes_pmtile() {
		let mut bboxes = vec![make_bbox(1, 1, 1), make_bbox(1, 0, 0), make_bbox(1, 0, 1)];
		// Compute expected by sorting by Hilbert index directly
		let mut expected = bboxes.clone();
		expected.sort_by_cached_key(|b| b.get_hilbert_index().unwrap());
		TraversalOrder::PMTiles.sort_bboxes(&mut bboxes, 1);
		assert_eq!(bboxes, expected, "PMTiles sort matches Hilbert index sort");
	}

	#[test]
	fn test_intersect_orders() {
		use TraversalOrder::*;
		let mut o = AnyOrder;
		o.intersect(&DepthFirst).unwrap();
		assert_eq!(o, DepthFirst);

		let mut o2 = PMTiles;
		o2.intersect(&AnyOrder).unwrap();
		assert_eq!(o2, PMTiles);

		let mut o3 = DepthFirst;
		let result = o3.intersect(&PMTiles);
		assert!(result.is_err(), "Merging DepthFirst with PMTiles should error");
	}
}
