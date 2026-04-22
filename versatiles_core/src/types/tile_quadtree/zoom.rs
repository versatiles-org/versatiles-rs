//! Zoom level traversal for [`TileQuadtree`].

use super::{Node, TileQuadtree};

impl TileQuadtree {
	/// Return a quadtree at the given zoom level
	#[must_use]
	pub fn at_level(&self, level: u8) -> TileQuadtree {
		use std::cmp::Ordering::{Equal, Greater, Less};
		match level.cmp(&self.level) {
			Equal => self.clone(),
			Less => TileQuadtree {
				level,
				root: reduce_max_depth(&self.root, level),
			},
			Greater => TileQuadtree {
				level,
				root: self.root.clone(),
			},
		}
	}
}

/// Trim a quadtree node to a maximum depth, merging all nodes below that depth into a single decision.
fn reduce_max_depth(node: &Node, max_depth: u8) -> Node {
	match node {
		Node::Empty => Node::Empty,
		Node::Full => Node::Full,
		Node::Partial(children) => {
			if max_depth == 0 {
				// Children are leaves — merge into one decision.
				// Semantics: covered at coarser level if ANY child is non-empty.
				let any_covered = children.iter().any(|c| !matches!(c, Node::Empty));
				if any_covered { Node::Full } else { Node::Empty }
			} else {
				Node::new_partial([
					reduce_max_depth(&children[0], max_depth - 1),
					reduce_max_depth(&children[1], max_depth - 1),
					reduce_max_depth(&children[2], max_depth - 1),
					reduce_max_depth(&children[3], max_depth - 1),
				])
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileBBox;
	use rstest::rstest;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	/// `at_level` preserves the requested level and never yields more tiles
	/// than the source when going coarser.
	#[rstest]
	#[case::coarser(3)]
	#[case::same(4)]
	#[case::finer(5)]
	fn at_level_preserves_requested_level(#[case] target: u8) {
		let t = TileQuadtree::from_bbox(&bbox(4, 4, 4, 11, 11));
		let out = t.at_level(target);
		assert_eq!(out.level(), target);
		if target < t.level() {
			assert!(
				out.count_tiles() <= t.count_tiles(),
				"coarser level must not grow tile count"
			);
		}
	}
}
