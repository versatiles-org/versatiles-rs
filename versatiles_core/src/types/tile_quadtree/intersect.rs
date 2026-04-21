use super::{BBox, Node, TileQuadtree};
use crate::{TileBBox, TileCover, TilePyramid, types::info_trait::TileCoverInfo};
use anyhow::Result;

impl TileQuadtree {
	/// Returns `true` if any tile covered by `self` falls within `bbox`.
	///
	/// Returns `false` if the zoom levels differ or either side is empty.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		if self.level != bbox.level() {
			return false;
		}
		let Some(bbox) = BBox::from_bbox(bbox) else {
			return false;
		};
		self.root.intersects_bbox((0, 0), 1u64 << self.level, bbox)
	}

	/// Returns `true` if any tile is covered by both `self` and `tree`.
	///
	/// Returns `false` if the zoom levels differ or either side is empty.
	#[must_use]
	pub fn intersects_tree(&self, tree: &TileQuadtree) -> bool {
		if self.level != tree.level() {
			return false;
		}
		self.root.intersects_tree(&tree.root)
	}

	/// Returns `true` if `self` shares at least one tile with `cover`.
	#[must_use]
	pub fn intersects_cover(&self, cover: &TileCover) -> bool {
		cover.intersects_tree(self)
	}

	/// Returns `true` if `self` shares at least one tile with the corresponding
	/// level of `pyramid`.
	#[must_use]
	pub fn intersects_pyramid(&self, pyramid: &TilePyramid) -> bool {
		self.intersects_cover(pyramid.level_ref(self.level))
	}

	/// Shrinks `self` in place to the tiles that also fall within `bbox`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		self.ensure_same_level(bbox, "intersect")?;
		let Some(bbox) = BBox::from_bbox(bbox) else {
			self.root = Node::Empty;
			return Ok(());
		};
		self.root.intersect_bbox((0, 0), 1u64 << self.level, &bbox);
		Ok(())
	}

	/// Shrinks `self` in place to the tiles also present in `tree`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_tree(&mut self, tree: &TileQuadtree) -> Result<()> {
		self.ensure_same_level(tree, "intersect")?;
		self.root.intersect_tree(&tree.root);
		Ok(())
	}

	/// Shrinks `self` in place to the tiles also present in `cover`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_cover(&mut self, cover: &TileCover) -> Result<()> {
		self.ensure_same_level(cover, "intersect")?;
		*self = cover.intersection_tree(self)?.into_tree();
		Ok(())
	}

	/// Shrinks `self` in place to the tiles also present in the corresponding
	/// level of `pyramid`.
	pub fn intersect_pyramid(&mut self, pyramid: &TilePyramid) {
		self.intersect_cover(pyramid.level_ref(self.level)).unwrap();
	}

	/// Returns a new quadtree containing only the tiles shared by `self` and
	/// `bbox`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_bbox(&self, bbox: &TileBBox) -> Result<Self> {
		self.ensure_same_level(bbox, "intersect")?;
		let root = if let Some(bbox) = BBox::from_bbox(bbox) {
			self.root.intersection_bbox((0, 0), 1u64 << self.level, bbox)
		} else {
			Node::Empty
		};
		Ok(TileQuadtree {
			level: self.level,
			root,
		})
	}

	/// Returns a new quadtree containing only the tiles shared by `self` and
	/// `tree`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_tree(&self, tree: &TileQuadtree) -> Result<Self> {
		self.ensure_same_level(tree, "intersect")?;
		Ok(TileQuadtree {
			level: self.level,
			root: self.root.intersection_tree(&tree.root),
		})
	}

	/// Returns a new quadtree containing only the tiles shared by `self` and
	/// `cover`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_cover(&self, cover: &TileCover) -> Result<Self> {
		self.ensure_same_level(cover, "intersect")?;
		Ok(cover.intersection_tree(self)?.into_tree())
	}

	/// Returns a new quadtree containing only the tiles shared by `self` and
	/// the corresponding level of `pyramid`.
	#[must_use]
	pub fn intersection_pyramid(&self, pyramid: &TilePyramid) -> Self {
		self.intersection_cover(pyramid.level_ref(self.level)).unwrap()
	}
}

impl Node {
	/// Returns `true` if the bbox overlaps with any tile in this subtree.
	///
	/// `(x_off, y_off)` and `size` describe the tile-space region this node
	/// covers; `bbox` uses exclusive max coordinates.
	pub fn intersects_bbox(&self, (x_off, y_off): (u64, u64), size: u64, bbox: BBox) -> bool {
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
					let x_min = bbox.x_min.max(cx);
					let y_min = bbox.y_min.max(cy);
					let x_max = bbox.x_max.min(cx_max);
					let y_max = bbox.y_max.min(cy_max);
					if x_min < x_max && y_min < y_max {
						// Pass the clipped sub-bbox so children don't re-clip unnecessarily
						let child_bbox = BBox::new(x_min, y_min, x_max, y_max);
						if child.intersects_bbox((cx, cy), half, child_bbox) {
							return true;
						}
					}
				}
				false
			}
		}
	}

	/// Returns `true` if any tile is covered by both `self` and `b`.
	pub fn intersects_tree(&self, b: &Node) -> bool {
		match (self, b) {
			(Node::Empty, _) | (_, Node::Empty) => false,
			(Node::Full, _) | (_, Node::Full) => true,
			(Node::Partial(ac), Node::Partial(bc)) => ac.iter().zip(bc.iter()).any(|(ac, bc)| ac.intersects_tree(bc)),
		}
	}

	/// Removes from this subtree all tiles that fall outside `bbox`.
	///
	/// `(x_off, y_off)` and `size` describe this node's tile-space region;
	/// `bbox` uses exclusive max coordinates.
	pub fn intersect_bbox(&mut self, (x_off, y_off): (u64, u64), size: u64, bbox: &BBox) {
		if self == &Node::Empty {
			return;
		}
		// Clip bbox to this cell's region.
		let ix_min = bbox.x_min.max(x_off);
		let iy_min = bbox.y_min.max(y_off);
		let ix_max = bbox.x_max.min(x_off + size);
		let iy_max = bbox.y_max.min(y_off + size);

		// No overlap → clear this subtree entirely.
		if ix_min >= ix_max || iy_min >= iy_max {
			*self = Node::Empty;
			return;
		}

		if self == &Node::Full {
			if ix_min == x_off && iy_min == y_off && ix_max == x_off + size && iy_max == y_off + size {
				// bbox covers the entire cell: stays Full.
				return;
			}
			// Materialise four Full children, then intersect each with bbox.
			*self = Node::new_partial_full();
		}

		if let Node::Partial(children) = self {
			let half = size / 2;
			let mid_x = x_off + half;
			let mid_y = y_off + half;
			children[0].intersect_bbox((x_off, y_off), half, bbox);
			children[1].intersect_bbox((mid_x, y_off), half, bbox);
			children[2].intersect_bbox((x_off, mid_y), half, bbox);
			children[3].intersect_bbox((mid_x, mid_y), half, bbox);
			self.normalize();
		}
	}

	/// Removes from this subtree all tiles not also present in `b`.
	pub fn intersect_tree(&mut self, b: &Node) {
		match (self, b) {
			(Node::Empty, _) | (_, Node::Full) => (),
			(s, Node::Empty) => *s = Node::Empty,
			(Node::Partial(ac), Node::Partial(bc)) => {
				ac[0].intersect_tree(&bc[0]);
				ac[1].intersect_tree(&bc[1]);
				ac[2].intersect_tree(&bc[2]);
				ac[3].intersect_tree(&bc[3]);
			}
			(s, other) => *s = other.clone(),
		}
	}

	/// Returns a new node covering only the tiles in this subtree that also
	/// fall within `bbox`.
	///
	/// `(x_off, y_off)` and `size` describe this node's tile-space region;
	/// `bbox` uses exclusive max coordinates.
	pub fn intersection_bbox(&self, (x_off, y_off): (u64, u64), size: u64, bbox: BBox) -> Node {
		match self {
			Node::Empty => Node::Empty,
			Node::Full => Node::build_node(
				u8::try_from(size.trailing_zeros()).unwrap(),
				(x_off, y_off),
				size,
				&bbox,
			),
			Node::Partial(children) => {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let offsets = [(x_off, y_off), (mid_x, y_off), (x_off, mid_y), (mid_x, mid_y)];
				let intersect = |i: usize| -> Node {
					let (cx, cy) = offsets[i];
					let ix_min = bbox.x_min.max(cx);
					let iy_min = bbox.y_min.max(cy);
					let ix_max = bbox.x_max.min(cx + half);
					let iy_max = bbox.y_max.min(cy + half);
					if ix_min < ix_max && iy_min < iy_max {
						children[i].intersection_bbox((cx, cy), half, BBox::new(ix_min, iy_min, ix_max, iy_max))
					} else {
						Node::Empty
					}
				};
				Node::new_partial([intersect(0), intersect(1), intersect(2), intersect(3)])
			}
		}
	}

	/// Returns a new node covering only the tiles present in both `self` and `b`.
	pub fn intersection_tree(&self, b: &Node) -> Node {
		match (self, b) {
			(Node::Empty, _) | (_, Node::Empty) => Node::Empty,
			(Node::Full, other) | (other, Node::Full) => other.clone(),
			(Node::Partial(ac), Node::Partial(bc)) => Node::new_partial([
				ac[0].intersection_tree(&bc[0]),
				ac[1].intersection_tree(&bc[1]),
				ac[2].intersection_tree(&bc[2]),
				ac[3].intersection_tree(&bc[3]),
			]),
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

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn intersects() {
		let a = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		let b = TileQuadtree::from_bbox(&bbox(3, 3, 3, 7, 7));
		let c = TileQuadtree::from_bbox(&bbox(3, 4, 4, 7, 7));
		assert!(a.intersects_tree(&b));
		assert!(!a.intersects_tree(&c));
		assert!(a.intersects_tree(&TileQuadtree::new_full(3).unwrap()));
		assert!(!a.intersects_tree(&TileQuadtree::new_empty(3).unwrap()));
	}

	#[test]
	fn intersect_bbox_clips_full_tree() -> Result<()> {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.intersect_bbox(&bbox(3, 2, 2, 5, 5))?;
		assert_eq!(t.count_tiles(), 16); // 4×4 inner square
		assert!(t.includes_coord(&coord(3, 2, 2)));
		assert!(t.includes_coord(&coord(3, 5, 5)));
		assert!(!t.includes_coord(&coord(3, 0, 0)));
		assert!(!t.includes_coord(&coord(3, 6, 6)));
		Ok(())
	}

	#[test]
	fn intersect_bbox_with_empty_bbox_clears_tree() -> Result<()> {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.intersect_bbox(&TileBBox::new_empty(3)?)?;
		assert!(t.is_empty());
		Ok(())
	}

	#[test]
	fn intersect_bbox_on_empty_tree_is_noop() -> Result<()> {
		let mut t = TileQuadtree::new_empty(3).unwrap();
		t.intersect_bbox(&bbox(3, 0, 0, 7, 7))?;
		assert!(t.is_empty());
		Ok(())
	}

	#[test]
	fn intersect_bbox_full_coverage_is_noop() -> Result<()> {
		let mut t = TileQuadtree::from_bbox(&bbox(3, 1, 1, 5, 5));
		let count_before = t.count_tiles();
		t.intersect_bbox(&TileBBox::new_full(3)?)?;
		assert_eq!(t.count_tiles(), count_before);
		Ok(())
	}

	#[test]
	fn intersect_bbox_partial_tree_clips_correctly() -> Result<()> {
		// Tree covers (0,0)-(7,3); clip to (4,0)-(7,7) → intersection is (4,0)-(7,3) = 4×4 = 16
		let mut t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 7, 3));
		t.intersect_bbox(&bbox(3, 4, 0, 7, 7))?;
		assert_eq!(t.count_tiles(), 16);
		assert!(!t.includes_coord(&coord(3, 3, 0))); // clipped left
		assert!(!t.includes_coord(&coord(3, 4, 4))); // clipped bottom
		assert!(t.includes_coord(&coord(3, 4, 0)));
		assert!(t.includes_coord(&coord(3, 7, 3)));
		Ok(())
	}

	#[test]
	fn intersect_bbox_zoom_mismatch_errors() {
		let mut t = TileQuadtree::new_full(3).unwrap();
		assert!(t.intersect_bbox(&bbox(4, 0, 0, 1, 1)).is_err());
	}
}
