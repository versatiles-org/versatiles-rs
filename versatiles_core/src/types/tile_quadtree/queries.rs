//! Query methods for [`TileQuadtree`].

use super::TileQuadtree;
use crate::types::tile_quadtree::node::Node;

impl TileQuadtree {
	/// Return true if the quadtree contains no tiles.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.root.is_empty()
	}

	/// Return true if the quadtree contains all tiles at its zoom level.
	#[must_use]
	pub fn is_full(&self) -> bool {
		self.root.is_full()
	}

	/// Count the total number of tiles in the quadtree.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		self.root.count_tiles(self.level)
	}

	/// Count the number of internal (Partial) nodes in the quadtree.
	#[must_use]
	pub fn count_nodes(&self) -> u64 {
		self.root.count_nodes()
	}
}

impl Node {
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

	/// Returns the number of leaf tiles covered by this subtree at the given
	/// remaining depth.
	pub fn count_tiles(&self, remaining_depth: u8) -> u64 {
		match self {
			Node::Empty => 0,
			Node::Full => 1u64 << (2 * u32::from(remaining_depth)),
			Node::Partial(children) => {
				if remaining_depth == 0 {
					// Shouldn't happen in a well-formed tree, but handle gracefully
					1
				} else {
					children.iter().map(|c| c.count_tiles(remaining_depth - 1)).sum()
				}
			}
		}
	}

	/// Returns the number of nodes (including this one) in the subtree.
	pub fn count_nodes(&self) -> u64 {
		match self {
			Node::Empty | Node::Full => 1,
			Node::Partial(children) => 1 + children.iter().map(Node::count_nodes).sum::<u64>(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileCoord;
	use rstest::rstest;

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	/// `count_tiles` on a full quadtree at zoom `z` equals `4^z` (2²ᶻ).
	#[rstest]
	#[case(0)]
	#[case(1)]
	#[case(2)]
	#[case(3)]
	#[case(4)]
	#[case(5)]
	fn tile_count_full(#[case] z: u8) {
		let expected = 1u64 << (2 * u32::from(z));
		assert_eq!(TileQuadtree::new_full(z).unwrap().count_tiles(), expected);
	}

	/// Structural tree kind → node count (empty/full collapse to 1 node;
	/// partial has more than one).
	#[rstest]
	#[case::empty(TileQuadtree::new_empty(4).unwrap(), 1, false)]
	#[case::full(TileQuadtree::new_full(5).unwrap(), 1, false)]
	#[case::partial({
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.insert_coord(&coord(3, 0, 0)).unwrap();
		t
	}, 1, true)]
	fn count_nodes_cases(#[case] t: TileQuadtree, #[case] min_nodes: u64, #[case] must_exceed: bool) {
		let n = t.count_nodes();
		if must_exceed {
			assert!(
				n > min_nodes,
				"partial tree must have more than {min_nodes} node(s), got {n}"
			);
		} else {
			assert_eq!(n, min_nodes);
		}
	}
}
