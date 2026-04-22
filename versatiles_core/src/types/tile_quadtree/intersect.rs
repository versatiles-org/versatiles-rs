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
		self.root.intersects_bbox(&BBox::root(self.level), &bbox)
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
		self.root.intersect_bbox(&BBox::root(self.level), &bbox);
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
		self
			.intersect_cover(pyramid.level_ref(self.level))
			.expect("same-level operation");
	}

	/// Returns a new quadtree containing only the tiles shared by `self` and
	/// `bbox`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_bbox(&self, bbox: &TileBBox) -> Result<Self> {
		self.ensure_same_level(bbox, "intersect")?;
		let root = if let Some(bbox) = BBox::from_bbox(bbox) {
			self.root.intersection_bbox(&BBox::root(self.level), &bbox)
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
		self
			.intersection_cover(pyramid.level_ref(self.level))
			.expect("same-level operation")
	}
}

impl Node {
	/// Returns `true` if the bbox overlaps with any tile in this subtree.
	///
	/// `cell` is the tile-space region this node covers; `bbox` uses exclusive
	/// max coordinates. The caller must ensure `cell.intersection(bbox)` is
	/// non-empty before calling.
	pub fn intersects_bbox(&self, cell: &BBox, bbox: &BBox) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let quads = cell.quadrants();
				children
					.iter()
					.zip(&quads)
					.any(|(child, q)| q.intersection(bbox).is_some() && child.intersects_bbox(q, bbox))
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
	/// `cell` is this node's tile-space region; `bbox` uses exclusive max
	/// coordinates.
	pub fn intersect_bbox(&mut self, cell: &BBox, bbox: &BBox) {
		if self == &Node::Empty {
			return;
		}
		// No overlap → clear this subtree entirely.
		if cell.intersection(bbox).is_none() {
			*self = Node::Empty;
			return;
		}
		if self == &Node::Full {
			// bbox covers the entire cell: stays Full.
			if bbox.covers(cell) {
				return;
			}
			// Materialise four Full children, then intersect each with bbox.
			*self = Node::new_partial_full();
		}
		if let Node::Partial(children) = self {
			let quads = cell.quadrants();
			for (child, q) in children.iter_mut().zip(&quads) {
				child.intersect_bbox(q, bbox);
			}
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
	/// `cell` is this node's tile-space region; `bbox` uses exclusive max
	/// coordinates.
	pub fn intersection_bbox(&self, cell: &BBox, bbox: &BBox) -> Node {
		match self {
			Node::Empty => Node::Empty,
			Node::Full => Node::build_node(cell, bbox),
			Node::Partial(children) => {
				let quads = cell.quadrants();
				Node::new_partial(std::array::from_fn(|i| {
					if quads[i].intersection(bbox).is_some() {
						children[i].intersection_bbox(&quads[i], bbox)
					} else {
						Node::Empty
					}
				}))
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
	use rstest::rstest;

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	// ── intersects_bbox ─────────────────────────────────────────────────────

	#[rstest]
	#[case::overlap(bbox(3, 2, 2, 5, 5), true)]
	#[case::disjoint(bbox(3, 4, 4, 7, 7), false)]
	#[case::zoom_mismatch(bbox(4, 0, 0, 1, 1), false)]
	#[case::empty_bbox(TileBBox::new_empty(3).unwrap(), false)]
	fn intersects_bbox_cases(#[case] other: TileBBox, #[case] expected: bool) {
		let t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		assert_eq!(t.intersects_bbox(&other), expected);
	}

	// ── intersects_tree ─────────────────────────────────────────────────────

	#[rstest]
	#[case::overlap(TileQuadtree::from_bbox(&bbox(3, 3, 3, 7, 7)), true)]
	#[case::disjoint(TileQuadtree::from_bbox(&bbox(3, 4, 4, 7, 7)), false)]
	#[case::full(TileQuadtree::new_full(3).unwrap(), true)]
	#[case::empty(TileQuadtree::new_empty(3).unwrap(), false)]
	#[case::zoom_mismatch(TileQuadtree::new_full(4).unwrap(), false)]
	fn intersects_tree_cases(#[case] other: TileQuadtree, #[case] expected: bool) {
		let t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		assert_eq!(t.intersects_tree(&other), expected);
	}

	// ── intersects_cover + intersects_pyramid ──────────────────────────────

	#[rstest]
	#[case::overlap(bbox(3, 2, 2, 5, 5), true)]
	#[case::disjoint(bbox(3, 4, 4, 7, 7), false)]
	fn intersects_cover_cases(#[case] other: TileBBox, #[case] expected: bool) {
		let t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		assert_eq!(t.intersects_cover(&TileCover::from(other)), expected);
	}

	#[test]
	fn intersects_pyramid_checks_same_level() {
		let t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(3, 2, 2, 5, 5)).unwrap();
		assert!(t.intersects_pyramid(&p));
		// A pyramid whose level 3 is empty does not intersect.
		assert!(!t.intersects_pyramid(&TilePyramid::new_empty()));
	}

	// ── intersect_bbox ──────────────────────────────────────────────────────

	/// Initial tree + clip bbox → expected tile count after clipping.
	#[rstest]
	#[case::clip_full_tree(TileQuadtree::new_full(3).unwrap(), bbox(3, 2, 2, 5, 5), 16)]
	#[case::empty_bbox_clears(TileQuadtree::new_full(3).unwrap(), TileBBox::new_empty(3).unwrap(), 0)]
	#[case::empty_tree_stays_empty(TileQuadtree::new_empty(3).unwrap(), bbox(3, 0, 0, 7, 7), 0)]
	#[case::full_coverage_noop(TileQuadtree::from_bbox(&bbox(3, 1, 1, 5, 5)), TileBBox::new_full(3).unwrap(), 25)]
	#[case::partial_tree(TileQuadtree::from_bbox(&bbox(3, 0, 0, 7, 3)), bbox(3, 4, 0, 7, 7), 16)]
	fn intersect_bbox_cases(
		#[case] mut t: TileQuadtree,
		#[case] clip: TileBBox,
		#[case] expected_count: u64,
	) -> Result<()> {
		t.intersect_bbox(&clip)?;
		assert_eq!(t.count_tiles(), expected_count);
		Ok(())
	}

	/// Spot-check corner inclusion after a partial-tree clip.
	#[test]
	fn intersect_bbox_partial_tree_corner_checks() -> Result<()> {
		let mut t = TileQuadtree::from_bbox(&bbox(3, 0, 0, 7, 3));
		t.intersect_bbox(&bbox(3, 4, 0, 7, 7))?;
		assert!(t.includes_coord(&coord(3, 4, 0)));
		assert!(t.includes_coord(&coord(3, 7, 3)));
		assert!(!t.includes_coord(&coord(3, 3, 0))); // clipped left
		assert!(!t.includes_coord(&coord(3, 4, 4))); // clipped bottom
		Ok(())
	}

	// ── intersect_tree ──────────────────────────────────────────────────────

	#[rstest]
	#[case::overlap(TileQuadtree::from_bbox(&bbox(3, 0, 0, 7, 3)), TileQuadtree::from_bbox(&bbox(3, 4, 0, 7, 7)), 16)]
	#[case::with_empty_clears(TileQuadtree::new_full(3).unwrap(), TileQuadtree::new_empty(3).unwrap(), 0)]
	#[case::with_full_noop(TileQuadtree::from_bbox(&bbox(3, 1, 1, 5, 5)), TileQuadtree::new_full(3).unwrap(), 25)]
	fn intersect_tree_cases(
		#[case] mut t: TileQuadtree,
		#[case] other: TileQuadtree,
		#[case] expected_count: u64,
	) -> Result<()> {
		t.intersect_tree(&other)?;
		assert_eq!(t.count_tiles(), expected_count);
		Ok(())
	}

	// ── intersect_cover + intersect_pyramid ─────────────────────────────────

	#[test]
	fn intersect_cover_narrows_tree() -> Result<()> {
		let mut t = TileQuadtree::new_full(3).unwrap();
		t.intersect_cover(&TileCover::from(bbox(3, 2, 2, 5, 5)))?;
		assert_eq!(t.count_tiles(), 16);
		Ok(())
	}

	#[test]
	fn intersect_pyramid_uses_same_level() {
		let mut t = TileQuadtree::new_full(3).unwrap();
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(3, 2, 2, 5, 5)).unwrap();
		t.intersect_pyramid(&p);
		assert_eq!(t.count_tiles(), 16);
	}

	// ── intersection_* (pure variants) ──────────────────────────────────────

	#[rstest]
	#[case::overlap(TileQuadtree::from_bbox(&bbox(3, 0, 0, 7, 3)), TileQuadtree::from_bbox(&bbox(3, 4, 0, 7, 7)), 16)]
	#[case::with_full_a(TileQuadtree::from_bbox(&bbox(3, 1, 1, 4, 4)), TileQuadtree::new_full(3).unwrap(), 16)]
	#[case::with_full_b(TileQuadtree::new_full(3).unwrap(), TileQuadtree::from_bbox(&bbox(3, 1, 1, 4, 4)), 16)]
	#[case::with_empty(TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3)), TileQuadtree::new_empty(3).unwrap(), 0)]
	fn intersection_tree_cases(#[case] a: TileQuadtree, #[case] b: TileQuadtree, #[case] expected: u64) -> Result<()> {
		let original_a_count = a.count_tiles();
		let original_b_count = b.count_tiles();
		let out = a.intersection_tree(&b)?;
		assert_eq!(out.count_tiles(), expected);
		// Inputs must remain unchanged — `intersection_tree` is pure.
		assert_eq!(a.count_tiles(), original_a_count);
		assert_eq!(b.count_tiles(), original_b_count);
		Ok(())
	}

	#[test]
	fn intersection_bbox_with_empty_bbox_yields_empty_tree() -> Result<()> {
		let t = TileQuadtree::new_full(3).unwrap();
		let out = t.intersection_bbox(&TileBBox::new_empty(3)?)?;
		assert!(out.is_empty());
		assert_eq!(out.level(), 3);
		Ok(())
	}

	#[test]
	fn intersection_cover_returns_subset() -> Result<()> {
		let t = TileQuadtree::new_full(3).unwrap();
		let out = t.intersection_cover(&TileCover::from(bbox(3, 1, 1, 4, 4)))?;
		assert_eq!(out.count_tiles(), 16);
		Ok(())
	}

	#[test]
	fn intersection_pyramid_returns_subset_at_self_level() {
		let t = TileQuadtree::new_full(3).unwrap();
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(3, 1, 1, 4, 4)).unwrap();
		assert_eq!(t.intersection_pyramid(&p).count_tiles(), 16);
	}

	// ── Zoom-mismatch across every method that validates levels ────────────

	/// Tag identifying the method under test.
	#[derive(Debug, Clone, Copy)]
	enum ZoomMismatchOp {
		IntersectBbox,
		IntersectTree,
		IntersectCover,
		IntersectionBbox,
		IntersectionTree,
		IntersectionCover,
	}

	#[rstest]
	#[case(ZoomMismatchOp::IntersectBbox)]
	#[case(ZoomMismatchOp::IntersectTree)]
	#[case(ZoomMismatchOp::IntersectCover)]
	#[case(ZoomMismatchOp::IntersectionBbox)]
	#[case(ZoomMismatchOp::IntersectionTree)]
	#[case(ZoomMismatchOp::IntersectionCover)]
	fn zoom_mismatch_errors(#[case] op: ZoomMismatchOp) {
		let mut t = TileQuadtree::new_full(3).unwrap();
		let mismatched_bbox = bbox(4, 0, 0, 1, 1);
		let mismatched_tree = TileQuadtree::new_full(4).unwrap();
		let mismatched_cover = TileCover::from(bbox(4, 0, 0, 1, 1));
		let result: Result<()> = match op {
			ZoomMismatchOp::IntersectBbox => t.intersect_bbox(&mismatched_bbox),
			ZoomMismatchOp::IntersectTree => t.intersect_tree(&mismatched_tree),
			ZoomMismatchOp::IntersectCover => t.intersect_cover(&mismatched_cover),
			ZoomMismatchOp::IntersectionBbox => t.intersection_bbox(&mismatched_bbox).map(|_| ()),
			ZoomMismatchOp::IntersectionTree => t.intersection_tree(&mismatched_tree).map(|_| ()),
			ZoomMismatchOp::IntersectionCover => t.intersection_cover(&mismatched_cover).map(|_| ()),
		};
		assert!(result.is_err(), "{op:?} should reject zoom mismatch");
	}

	// ── Node-level coverage: Partial-tree normalize path ───────────────────

	#[test]
	fn intersect_bbox_over_partial_tree_normalizes() -> Result<()> {
		// Non-adjacent inserts produce a Partial root; clipping one quadrant
		// away exercises `normalize()` inside `Node::intersect_bbox`.
		let mut t = TileQuadtree::new_empty(4).unwrap();
		t.insert_bbox(&bbox(4, 0, 0, 3, 3))?;
		t.insert_bbox(&bbox(4, 12, 12, 15, 15))?;
		t.intersect_bbox(&bbox(4, 0, 0, 3, 3))?;
		assert_eq!(t.count_tiles(), 16);
		assert!(!t.includes_coord(&coord(4, 12, 12)));
		Ok(())
	}
}
