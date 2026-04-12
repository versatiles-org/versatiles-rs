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

/// A compact axis-aligned bounding box used internally by recursive quadtree helpers.
///
/// All coordinates are in tile-space and the max values are exclusive.
#[derive(Clone, Copy)]
pub(crate) struct BBox {
	pub x_min: u64,
	pub y_min: u64,
	pub x_max: u64,
	pub y_max: u64,
}

/// A set of tiles at a single zoom level, backed by a quadtree.
///
/// # Examples
/// ```
/// use versatiles_core::TileQuadtree;
///
/// let tree = TileQuadtree::new_empty(5);
/// assert!(tree.is_empty());
///
/// let full = TileQuadtree::new_full(3);
/// assert!(full.is_full());
/// assert_eq!(full.tile_count(), 64); // 8×8 tiles at zoom 3
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TileQuadtree {
	zoom: u8,
	root: Node,
}

impl TileQuadtree {
	/// Return the zoom level of this quadtree.
	#[must_use]
	pub fn zoom(&self) -> u8 {
		self.zoom
	}
}
