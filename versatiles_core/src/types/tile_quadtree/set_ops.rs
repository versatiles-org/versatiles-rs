//! Set operations for [`TileQuadtree`]: union, intersection, difference.

use super::{Node, TileQuadtree};
use anyhow::{Result, ensure};
use versatiles_derive::context;

impl TileQuadtree {
	/// Return the union of `self` and `other` (tiles in either).
	///
	/// # Errors
	/// Returns an error if zoom levels differ.
	#[context("Failed to union TileQuadtrees at levels {} and {}", self.level, other.level)]
	pub fn union(&self, other: &TileQuadtree) -> Result<TileQuadtree> {
		ensure!(
			self.level == other.level,
			"Cannot union quadtrees with different zoom levels: {} vs {}",
			self.level,
			other.level
		);
		let root = node_union(&self.root, &other.root);
		Ok(TileQuadtree {
			level: self.level,
			root,
		})
	}

	/// Return the intersection of `self` and `other` (tiles in both).
	///
	/// # Errors
	/// Returns an error if zoom levels differ.
	#[context("Failed to intersect TileQuadtrees at levels {} and {}", self.level, other.level)]
	pub fn intersection(&self, other: &TileQuadtree) -> Result<TileQuadtree> {
		ensure!(
			self.level == other.level,
			"Cannot intersect quadtrees with different zoom levels: {} vs {}",
			self.level,
			other.level
		);
		let root = node_intersection(&self.root, &other.root);
		Ok(TileQuadtree {
			level: self.level,
			root,
		})
	}

	/// Return the difference of `self` minus `other` (tiles in self but not other).
	///
	/// # Errors
	/// Returns an error if zoom levels differ.
	#[context("Failed to compute difference of TileQuadtrees at levels {} and {}", self.level, other.level)]
	pub fn difference(&self, other: &TileQuadtree) -> Result<TileQuadtree> {
		ensure!(
			self.level == other.level,
			"Cannot difference quadtrees with different zoom levels: {} vs {}",
			self.level,
			other.level
		);
		let root = node_difference(&self.root, &other.root);
		Ok(TileQuadtree {
			level: self.level,
			root,
		})
	}
}

fn node_union(a: &Node, b: &Node) -> Node {
	match (a, b) {
		(Node::Full, _) | (_, Node::Full) => Node::Full,
		(Node::Empty, x) | (x, Node::Empty) => x.clone(),
		(Node::Partial(ac), Node::Partial(bc)) => Node::new_partial([
			node_union(&ac[0], &bc[0]),
			node_union(&ac[1], &bc[1]),
			node_union(&ac[2], &bc[2]),
			node_union(&ac[3], &bc[3]),
		]),
	}
}

fn node_intersection(a: &Node, b: &Node) -> Node {
	match (a, b) {
		(Node::Empty, _) | (_, Node::Empty) => Node::Empty,
		(Node::Full, x) | (x, Node::Full) => x.clone(),
		(Node::Partial(ac), Node::Partial(bc)) => Node::new_partial([
			node_intersection(&ac[0], &bc[0]),
			node_intersection(&ac[1], &bc[1]),
			node_intersection(&ac[2], &bc[2]),
			node_intersection(&ac[3], &bc[3]),
		]),
	}
}

fn node_difference(a: &Node, b: &Node) -> Node {
	match (a, b) {
		(Node::Empty, _) | (_, Node::Full) => Node::Empty,
		(a, Node::Empty) => a.clone(),
		(Node::Full, Node::Partial(bc)) => {
			// Full minus partial: invert the partial
			Node::new_partial([
				node_difference(&Node::Full, &bc[0]),
				node_difference(&Node::Full, &bc[1]),
				node_difference(&Node::Full, &bc[2]),
				node_difference(&Node::Full, &bc[3]),
			])
		}
		(Node::Partial(ac), Node::Partial(bc)) => Node::new_partial([
			node_difference(&ac[0], &bc[0]),
			node_difference(&ac[1], &bc[1]),
			node_difference(&ac[2], &bc[2]),
			node_difference(&ac[3], &bc[3]),
		]),
	}
}
