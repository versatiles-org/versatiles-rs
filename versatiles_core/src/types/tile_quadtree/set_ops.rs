//! Set operations for [`TileQuadtree`]: union, intersection, difference.

use super::{Node, TileQuadtree};
use anyhow::{Result, ensure};

impl TileQuadtree {
	/// Return the union of `self` and `other` (tiles in either).
	///
	/// # Errors
	/// Returns an error if zoom levels differ.
	pub fn union(&self, other: &TileQuadtree) -> Result<TileQuadtree> {
		ensure!(
			self.zoom == other.zoom,
			"Cannot union quadtrees with different zoom levels: {} vs {}",
			self.zoom,
			other.zoom
		);
		let root = node_union(&self.root, &other.root);
		Ok(TileQuadtree { zoom: self.zoom, root })
	}

	/// Return the intersection of `self` and `other` (tiles in both).
	///
	/// # Errors
	/// Returns an error if zoom levels differ.
	pub fn intersection(&self, other: &TileQuadtree) -> Result<TileQuadtree> {
		ensure!(
			self.zoom == other.zoom,
			"Cannot intersect quadtrees with different zoom levels: {} vs {}",
			self.zoom,
			other.zoom
		);
		let root = node_intersection(&self.root, &other.root);
		Ok(TileQuadtree { zoom: self.zoom, root })
	}

	/// Return the difference of `self` minus `other` (tiles in self but not other).
	///
	/// # Errors
	/// Returns an error if zoom levels differ.
	pub fn difference(&self, other: &TileQuadtree) -> Result<TileQuadtree> {
		ensure!(
			self.zoom == other.zoom,
			"Cannot difference quadtrees with different zoom levels: {} vs {}",
			self.zoom,
			other.zoom
		);
		let root = node_difference(&self.root, &other.root);
		Ok(TileQuadtree { zoom: self.zoom, root })
	}
}

fn node_union(a: &Node, b: &Node) -> Node {
	match (a, b) {
		(Node::Full, _) | (_, Node::Full) => Node::Full,
		(Node::Empty, x) | (x, Node::Empty) => x.clone(),
		(Node::Partial(ac), Node::Partial(bc)) => {
			let children = [
				node_union(&ac[0], &bc[0]),
				node_union(&ac[1], &bc[1]),
				node_union(&ac[2], &bc[2]),
				node_union(&ac[3], &bc[3]),
			];
			Node::normalize(children)
		}
	}
}

fn node_intersection(a: &Node, b: &Node) -> Node {
	match (a, b) {
		(Node::Empty, _) | (_, Node::Empty) => Node::Empty,
		(Node::Full, x) | (x, Node::Full) => x.clone(),
		(Node::Partial(ac), Node::Partial(bc)) => {
			let children = [
				node_intersection(&ac[0], &bc[0]),
				node_intersection(&ac[1], &bc[1]),
				node_intersection(&ac[2], &bc[2]),
				node_intersection(&ac[3], &bc[3]),
			];
			Node::normalize(children)
		}
	}
}

fn node_difference(a: &Node, b: &Node) -> Node {
	match (a, b) {
		(Node::Empty, _) | (_, Node::Full) => Node::Empty,
		(a, Node::Empty) => a.clone(),
		(Node::Full, Node::Partial(bc)) => {
			// Full minus partial: invert the partial
			let children = [
				node_difference(&Node::Full, &bc[0]),
				node_difference(&Node::Full, &bc[1]),
				node_difference(&Node::Full, &bc[2]),
				node_difference(&Node::Full, &bc[3]),
			];
			Node::normalize(children)
		}
		(Node::Partial(ac), Node::Partial(bc)) => {
			let children = [
				node_difference(&ac[0], &bc[0]),
				node_difference(&ac[1], &bc[1]),
				node_difference(&ac[2], &bc[2]),
				node_difference(&ac[3], &bc[3]),
			];
			Node::normalize(children)
		}
	}
}
