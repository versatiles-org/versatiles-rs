//! A quadtree-based tile set for efficient spatial operations.
//!
//! [`TileQuadtree`] represents a set of tiles at a fixed zoom level using a
//! quadtree structure that compresses uniform regions to single nodes.
//!
//! # Node convention
//! Each `Partial` node stores four children in `[NW, NE, SW, SE]` order:
//! - Index 0 = NW (x < mid, y < mid)
//! - Index 1 = NE (x >= mid, y < mid)
//! - Index 2 = SW (x < mid, y >= mid)
//! - Index 3 = SE (x >= mid, y >= mid)

mod constructors;
mod fmt;
mod iter;
mod mutate;
mod node;
mod queries;
mod serialize;
mod set_ops;
#[cfg(test)]
mod tests;
mod zoom;

use node::Node;

use crate::TileBBox;

/// A compact axis-aligned bounding box used internally by recursive quadtree helpers.
///
/// All coordinates are in tile-space and the max values are exclusive.
#[derive(Clone, Copy)]
pub(super) struct BBox {
	pub x_min: u64,
	pub y_min: u64,
	pub x_max: u64,
	pub y_max: u64,
}

impl BBox {
	fn new(bbox: &TileBBox) -> Option<Self> {
		if bbox.is_empty() {
			return None;
		}
		Some(Self {
			x_min: u64::from(bbox.x_min().unwrap()),
			y_min: u64::from(bbox.y_min().unwrap()),
			x_max: u64::from(bbox.x_max().unwrap()) + 1,
			y_max: u64::from(bbox.y_max().unwrap()) + 1,
		})
	}
}

/// A set of tiles at a single zoom level, backed by a quadtree.
///
/// # Examples
/// ```
/// use versatiles_core::TileQuadtree;
///
/// let tree = TileQuadtree::new_empty(5).unwrap();
/// assert!(tree.is_empty());
///
/// let full = TileQuadtree::new_full(3).unwrap();
/// assert!(full.is_full());
/// assert_eq!(full.count_tiles(), 64); // 8×8 tiles at zoom 3
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TileQuadtree {
	level: u8,
	root: Node,
}

impl TileQuadtree {
	/// Return the zoom level of this quadtree.
	#[must_use]
	pub fn level(&self) -> u8 {
		self.level
	}
}
