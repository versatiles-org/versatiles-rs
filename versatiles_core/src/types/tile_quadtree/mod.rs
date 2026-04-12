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

	fn count(&self, remaining_depth: u8) -> u64 {
		match self {
			Node::Empty => 0,
			Node::Full => 1u64 << (2 * u32::from(remaining_depth)),
			Node::Partial(children) => {
				if remaining_depth == 0 {
					// Shouldn't happen in a well-formed tree, but handle gracefully
					1
				} else {
					children.iter().map(|c| c.count(remaining_depth - 1)).sum()
				}
			}
		}
	}

	fn count_partial(&self) -> u64 {
		match self {
			Node::Empty | Node::Full => 1,
			Node::Partial(children) => 1 + children.iter().map(Node::count_partial).sum::<u64>(),
		}
	}

	/// Returns the bounding box `(x_min, y_min, x_max_excl, y_max_excl)` of non-empty tiles.
	fn bounds(&self, x_off: u64, y_off: u64, size: u64) -> Option<(u64, u64, u64, u64)> {
		match self {
			Node::Empty => None,
			Node::Full => Some((x_off, y_off, x_off + size, y_off + size)),
			Node::Partial(children) => {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let child_offsets = [(x_off, y_off), (mid_x, y_off), (x_off, mid_y), (mid_x, mid_y)];
				let mut result: Option<(u64, u64, u64, u64)> = None;
				for (i, child) in children.iter().enumerate() {
					let (cx, cy) = child_offsets[i];
					if let Some(b) = child.bounds(cx, cy, half) {
						result = Some(match result {
							None => b,
							Some(r) => (r.0.min(b.0), r.1.min(b.1), r.2.max(b.2), r.3.max(b.3)),
						});
					}
				}
				result
			}
		}
	}

	fn contains_tile(&self, x_off: u64, y_off: u64, size: u64, tx: u64, ty: u64) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let (idx, cx, cy, half) = child_quadrant(x_off, y_off, size, tx, ty);
				children[idx].contains_tile(cx, cy, half, tx, ty)
			}
		}
	}

	fn contains_bbox(&self, x_off: u64, y_off: u64, size: u64, bbox: BBox) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let child_offsets = [(x_off, y_off), (mid_x, y_off), (x_off, mid_y), (mid_x, mid_y)];
				for (i, child) in children.iter().enumerate() {
					let (cx, cy) = child_offsets[i];
					let cx_max = cx + half;
					let cy_max = cy + half;
					// Clip bbox against this child's region
					let ix_min = bbox.x_min.max(cx);
					let iy_min = bbox.y_min.max(cy);
					let ix_max = bbox.x_max.min(cx_max);
					let iy_max = bbox.y_max.min(cy_max);
					if ix_min < ix_max && iy_min < iy_max {
						// Pass the clipped sub-bbox so children don't re-clip unnecessarily
						let child_bbox = BBox {
							x_min: ix_min,
							y_min: iy_min,
							x_max: ix_max,
							y_max: iy_max,
						};
						if !child.contains_bbox(cx, cy, half, child_bbox) {
							return false;
						}
					}
				}
				true
			}
		}
	}

	fn intersects(&self, b: &Node) -> bool {
		match (self, b) {
			(Node::Empty, _) | (_, Node::Empty) => false,
			(Node::Full, _) | (_, Node::Full) => true,
			(Node::Partial(ac), Node::Partial(bc)) => ac.iter().zip(bc.iter()).any(|(ac, bc)| ac.intersects(bc)),
		}
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
