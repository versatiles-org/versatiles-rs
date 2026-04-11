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
mod queries;
mod serialize;
mod set_ops;
#[cfg(test)]
mod tests;
mod zoom;

/// A single node in the quadtree.
///
/// - `Empty` — no tiles covered in this subtree.
/// - `Full`  — all tiles covered in this subtree.
/// - `Partial` — some tiles covered; children are `[NW, NE, SW, SE]`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
	Empty,
	Full,
	Partial(Box<[Node; 4]>),
}

impl Node {
	/// Normalize: collapse `Partial` where all children are the same.
	#[must_use]
	pub(crate) fn normalize(children: [Node; 4]) -> Node {
		if children.iter().all(|c| matches!(c, Node::Full)) {
			return Node::Full;
		}
		if children.iter().all(|c| matches!(c, Node::Empty)) {
			return Node::Empty;
		}
		Node::Partial(Box::new(children))
	}

	/// Return true if all tiles in this node's subtree are covered.
	#[must_use]
	pub(crate) fn is_full(&self) -> bool {
		matches!(self, Node::Full)
	}

	/// Return true if no tiles in this node's subtree are covered.
	#[must_use]
	pub(crate) fn is_empty(&self) -> bool {
		matches!(self, Node::Empty)
	}
}

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

/// Determine which child quadrant contains `(tx, ty)` and return
/// `(child_index, child_x_off, child_y_off, half_size)`.
///
/// Child indices follow `[NW, NE, SW, SE]` order (index 0..3).
pub(crate) fn child_quadrant(x_off: u64, y_off: u64, size: u64, tx: u64, ty: u64) -> (usize, u64, u64, u64) {
	let half = size / 2;
	let mid_x = x_off + half;
	let mid_y = y_off + half;
	if tx < mid_x {
		if ty < mid_y {
			(0, x_off, y_off, half)
		} else {
			(2, x_off, mid_y, half)
		}
	} else if ty < mid_y {
		(1, mid_x, y_off, half)
	} else {
		(3, mid_x, mid_y, half)
	}
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
