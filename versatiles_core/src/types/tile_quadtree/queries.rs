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
	use anyhow::Result;

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	#[test]
	fn tile_count_full() {
		for z in 0u8..=5 {
			let expected = 1u64 << (2 * u32::from(z));
			assert_eq!(TileQuadtree::new_full(z).unwrap().count_tiles(), expected);
		}
	}

	#[test]
	fn count_nodes_empty_and_full() {
		// An empty tree has 1 node (the root Empty node).
		assert_eq!(TileQuadtree::new_empty(4).unwrap().count_nodes(), 1);
		// A full tree has 1 node (the root Full node).
		assert_eq!(TileQuadtree::new_full(5).unwrap().count_nodes(), 1);
	}

	#[test]
	fn count_nodes_partial_tree() -> Result<()> {
		// A tree with a partial subtree has more than 1 node.
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.insert_coord(&coord(3, 0, 0))?;
		assert!(t.count_nodes() > 1, "partial tree should have more than one node");
		Ok(())
	}
}
