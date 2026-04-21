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
			.includes_coord((0, 0), 1u64 << self.level, (u64::from(coord.x), u64::from(coord.y)))
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
		self.root.includes_bbox((0, 0), 1u64 << self.level, bbox)
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
	/// `(x_off, y_off)` and `size` describe the tile-space region this node
	/// covers.
	pub fn includes_coord(&self, (x_off, y_off): (u64, u64), size: u64, (tx, ty): (u64, u64)) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let (idx, cx, cy, half) = Node::child_quadrant((x_off, y_off), size, (tx, ty));
				children[idx].includes_coord((cx, cy), half, (tx, ty))
			}
		}
	}

	/// Returns `true` if every tile in `bbox` is covered by this subtree.
	///
	/// `(x_off, y_off)` and `size` describe this node's tile-space region;
	/// `bbox` uses exclusive max coordinates.
	pub fn includes_bbox(&self, (x_off, y_off): (u64, u64), size: u64, bbox: BBox) -> bool {
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
						let child_bbox = BBox {
							x_min: ix_min,
							y_min: iy_min,
							x_max: ix_max,
							y_max: iy_max,
						};
						if !child.includes_bbox((cx, cy), half, child_bbox) {
							return false;
						}
					}
				}
				true
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
