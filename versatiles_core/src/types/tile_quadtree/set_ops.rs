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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileBBox;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn union() -> Result<()> {
		let a = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 7));
		let b = TileQuadtree::from_bbox(&bbox(3, 4, 0, 7, 7));
		let u = a.union(&b)?;
		assert!(u.is_full());
		Ok(())
	}

	#[test]
	fn intersection() -> Result<()> {
		let a = TileQuadtree::from_bbox(&bbox(3, 0, 0, 5, 5));
		let b = TileQuadtree::from_bbox(&bbox(3, 3, 3, 7, 7));
		let i = a.intersection(&b)?;
		// Overlap is [3,3] to [5,5] = 3x3 = 9 tiles
		assert_eq!(i.count_tiles(), 9);
		Ok(())
	}

	#[test]
	fn difference() -> Result<()> {
		let a = TileQuadtree::new_full(2).unwrap();
		let b = TileQuadtree::from_bbox(&bbox(2, 0, 0, 1, 1));
		let d = a.difference(&b)?;
		assert_eq!(d.count_tiles(), 12);
		Ok(())
	}

	#[test]
	fn set_ops_zoom_mismatch() {
		let a = TileQuadtree::new_full(3).unwrap();
		let b = TileQuadtree::new_full(4).unwrap();
		assert!(a.union(&b).is_err());
		assert!(a.intersection(&b).is_err());
		assert!(a.difference(&b).is_err());
	}
}
