use super::{BBox, Node, TileQuadtree};
use crate::{TileBBox, TileCoord, TileCover};

impl TileQuadtree {
	/// Returns `true` if `coord` is covered by this quadtree.
	///
	/// # Panics
	/// Panics if `coord` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		assert_eq!(self.level, coord.level);
		self
			.root
			.includes_coord(&BBox::root(self.level), (u64::from(coord.x), u64::from(coord.y)))
	}

	/// Returns `true` if every tile in `bbox` is covered by this quadtree.
	///
	/// An empty `bbox` is vacuously included (returns `true`).
	///
	/// # Panics
	/// Panics if `bbox` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_bbox(&self, bbox: &TileBBox) -> bool {
		assert_eq!(self.level, bbox.level());
		let Some(bbox) = BBox::from_bbox(bbox) else {
			return true;
		};
		self.root.includes_bbox(&BBox::root(self.level), &bbox)
	}

	/// Returns `true` if every tile in `tree` is also covered by `self`.
	///
	/// # Panics
	/// Panics if `tree` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_tree(&self, tree: &TileQuadtree) -> bool {
		assert_eq!(self.level, tree.level());
		self.root.includes_tree(&tree.root)
	}

	/// Returns `true` if every tile in `cover` is also covered by `self`.
	///
	/// Delegates to `includes_bbox` via `cover.to_bbox()`.
	#[must_use]
	pub fn includes_cover(&self, cover: &TileCover) -> bool {
		self.includes_bbox(&cover.to_bbox())
	}
}

impl Node {
	/// Returns `true` if every tile in `b`'s subtree is also in `self`.
	pub fn includes_tree(&self, b: &Node) -> bool {
		match (self, b) {
			(Node::Empty, Node::Full | Node::Partial(_)) => false,
			(Node::Empty | Node::Partial(_), Node::Empty) | (Node::Full, _) => true,
			(Node::Partial(ac), Node::Full) => ac.iter().all(Node::is_full),
			(Node::Partial(ac), Node::Partial(bc)) => ac.iter().zip(bc.iter()).all(|(ac, bc)| ac.includes_tree(bc)),
		}
	}

	/// Returns `true` if tile `(tx, ty)` is covered by this subtree.
	///
	/// `cell` is the tile-space region this node covers.
	pub fn includes_coord(&self, cell: &BBox, (tx, ty): (u64, u64)) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let (idx, child_cell) = Node::child_quadrant(cell, (tx, ty));
				children[idx].includes_coord(&child_cell, (tx, ty))
			}
		}
	}

	/// Returns `true` if every tile in `bbox` is covered by this subtree.
	///
	/// `cell` is this node's tile-space region; `bbox` uses exclusive max
	/// coordinates.
	pub fn includes_bbox(&self, cell: &BBox, bbox: &BBox) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let quads = cell.quadrants();
				children
					.iter()
					.zip(&quads)
					.all(|(child, q)| q.intersection(bbox).is_none() || child.includes_bbox(q, bbox))
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn includes_coord() {
		let t = TileQuadtree::from_bbox(&bbox(3, 2, 2, 4, 4));
		assert!(t.includes_coord(&coord(3, 2, 2)));
		assert!(t.includes_coord(&coord(3, 4, 4)));
		assert!(!t.includes_coord(&coord(3, 0, 0)));
		assert!(!t.includes_coord(&coord(3, 5, 5)));
	}

	#[test]
	#[should_panic(expected = "assertion `left == right` failed")]
	fn includes_coord_wrong_zoom_panics() {
		let t = TileQuadtree::from_bbox(&bbox(3, 2, 2, 4, 4));
		let _ = t.includes_coord(&coord(4, 2, 2));
	}

	#[test]
	fn includes_bbox() -> Result<()> {
		let full = TileQuadtree::new_full(3).unwrap();
		assert!(full.includes_bbox(&TileBBox::new_full(3)?));
		assert!(full.includes_bbox(&bbox(3, 0, 0, 3, 3)));

		let t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		assert!(t.includes_bbox(&bbox(3, 0, 0, 2, 2)));
		assert!(!t.includes_bbox(&TileBBox::new_full(3)?));
		Ok(())
	}

	#[test]
	#[should_panic(expected = "assertion `left == right` failed")]
	fn zoom_mismatch_includes_bbox() {
		let t = TileQuadtree::new_full(3).unwrap();
		let _ = t.includes_bbox(&bbox(4, 0, 0, 1, 1));
	}

	#[test]
	fn includes_empty_bbox_returns_true() -> Result<()> {
		// An empty bbox is trivially contained in any tree.
		let t = TileQuadtree::new_empty(4).unwrap();
		assert!(t.includes_bbox(&TileBBox::new_empty(4)?));
		Ok(())
	}
}
