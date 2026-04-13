//! Zoom level traversal for [`TileQuadtree`].

use super::{Node, TileQuadtree};

impl TileQuadtree {
	/// Return a new quadtree at zoom level `zoom - 1` (coarser).
	///
	/// A tile at the coarser level is marked as covered if ANY of its four
	/// sub-tiles at the current zoom are covered.
	///
	/// If this quadtree is already at zoom 0, returns a clone unchanged.
	#[must_use]
	pub fn level_up(&self) -> TileQuadtree {
		if self.level == 0 {
			return self.clone();
		}
		let root = node_level_up(&self.root, self.level);
		TileQuadtree {
			level: self.level - 1,
			root,
		}
	}

	/// Return a new quadtree at zoom level `zoom + 1` (finer).
	///
	/// Because the quadtree uses compressed representation, level_down simply
	/// increments the zoom counter — Full/Empty leaves automatically represent
	/// all tiles at the finer resolution.
	///
	/// # Panics
	/// Panics if zoom == 30 (would overflow u8).
	#[must_use]
	pub fn level_down(&self) -> TileQuadtree {
		assert!(self.level < 30, "Cannot level_down from zoom 30");
		TileQuadtree {
			level: self.level + 1,
			root: self.root.clone(),
		}
	}

	/// Return a quadtree at the given zoom level by repeatedly calling
	/// `level_up` or `level_down`.
	#[must_use]
	pub fn at_level(&self, zoom: u8) -> TileQuadtree {
		use std::cmp::Ordering::{Equal, Greater, Less};
		match zoom.cmp(&self.level) {
			Equal => self.clone(),
			Less => {
				let mut result = self.clone();
				while result.level > zoom {
					result = result.level_up();
				}
				result
			}
			Greater => {
				let mut result = self.clone();
				while result.level < zoom {
					result = result.level_down();
				}
				result
			}
		}
	}
}

/// Trim one level from the bottom of the tree.
/// `remaining_depth` = current zoom (depth from root to leaves).
fn node_level_up(node: &Node, remaining_depth: u8) -> Node {
	match node {
		Node::Empty => Node::Empty,
		Node::Full => Node::Full,
		Node::Partial(children) => {
			if remaining_depth == 1 {
				// Children are leaves — merge into one decision.
				// Semantics: covered at coarser level if ANY child is non-empty.
				let any_covered = children.iter().any(|c| !matches!(c, Node::Empty));
				if any_covered { Node::Full } else { Node::Empty }
			} else {
				Node::new_partial([
					node_level_up(&children[0], remaining_depth - 1),
					node_level_up(&children[1], remaining_depth - 1),
					node_level_up(&children[2], remaining_depth - 1),
					node_level_up(&children[3], remaining_depth - 1),
				])
			}
		}
	}
}
