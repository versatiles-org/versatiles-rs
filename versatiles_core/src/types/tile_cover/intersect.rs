use anyhow::Result;

use crate::{TileBBox, TileCover, TilePyramid, TileQuadtree};

impl TileCover {
	/// Returns `true` if `self` and `bbox` share at least one tile.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		match self {
			TileCover::Bbox(b) => b.intersects_bbox(bbox),
			TileCover::Tree(t) => t.intersects_bbox(bbox),
		}
	}

	/// Returns `true` if `self` and `tree` share at least one tile.
	#[must_use]
	pub fn intersects_tree(&self, tree: &TileQuadtree) -> bool {
		match self {
			TileCover::Bbox(b) => b.intersects_tree(tree),
			TileCover::Tree(t) => t.intersects_tree(tree),
		}
	}

	/// Returns `true` if `self` and `cover` share at least one tile.
	#[must_use]
	pub fn intersects_cover(&self, cover: &TileCover) -> bool {
		match (self, cover) {
			(TileCover::Bbox(b1), TileCover::Bbox(b2)) => b1.intersects_bbox(b2),
			(TileCover::Bbox(b), TileCover::Tree(t)) | (TileCover::Tree(t), TileCover::Bbox(b)) => t.intersects_bbox(b),
			(TileCover::Tree(t1), TileCover::Tree(t2)) => t1.intersects_tree(t2),
		}
	}

	/// Returns `true` if `self` shares at least one tile with the corresponding
	/// level of `pyramid`.
	#[must_use]
	pub fn intersects_pyramid(&self, pyramid: &TilePyramid) -> bool {
		self.intersects_cover(pyramid.level_ref(self.level()))
	}

	/// Shrinks `self` in place to the tiles also present in `bbox`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		match self {
			TileCover::Bbox(b) => b.intersect_bbox(bbox),
			TileCover::Tree(t) => t.intersect_bbox(bbox),
		}
	}

	/// Shrinks `self` in place to the tiles also present in `tree`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_tree(&mut self, tree: &TileQuadtree) -> Result<()> {
		match self {
			TileCover::Bbox(b) => b.intersect_tree(tree),
			TileCover::Tree(t) => t.intersect_tree(tree),
		}
	}

	/// Shrinks `self` in place to the tiles also present in `cover`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_cover(&mut self, cover: &TileCover) -> Result<()> {
		match (self, cover) {
			(TileCover::Bbox(b1), TileCover::Bbox(b2)) => b1.intersect_bbox(b2),
			(TileCover::Bbox(b), TileCover::Tree(t)) => b.intersect_tree(t),
			(TileCover::Tree(t), TileCover::Bbox(b)) => t.intersect_bbox(b),
			(TileCover::Tree(t1), TileCover::Tree(t2)) => t1.intersect_tree(t2),
		}
	}

	/// Shrinks `self` in place to the tiles also present in the corresponding
	/// level of `pyramid`.
	pub fn intersect_pyramid(&mut self, pyramid: &TilePyramid) {
		self
			.intersect_cover(pyramid.level_ref(self.level()))
			.expect("same-level operation");
	}

	/// Returns a new cover containing only the tiles shared by `self` and `bbox`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_bbox(&self, bbox: &TileBBox) -> Result<Self> {
		Ok(match self {
			TileCover::Bbox(b) => TileCover::from(b.intersection_bbox(bbox)?),
			TileCover::Tree(t) => TileCover::from(t.intersection_bbox(bbox)?),
		})
	}

	/// Returns a new cover containing only the tiles shared by `self` and `tree`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_tree(&self, tree: &TileQuadtree) -> Result<Self> {
		Ok(match self {
			TileCover::Bbox(b) => TileCover::from(b.intersection_tree(tree)?),
			TileCover::Tree(t) => TileCover::from(t.intersection_tree(tree)?),
		})
	}

	/// Returns a new cover containing only the tiles shared by `self` and `cover`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_cover(&self, cover: &TileCover) -> Result<Self> {
		Ok(match (self, cover) {
			(TileCover::Bbox(b1), TileCover::Bbox(b2)) => TileCover::from(b1.intersection_bbox(b2)?),
			(TileCover::Bbox(b), TileCover::Tree(t)) | (TileCover::Tree(t), TileCover::Bbox(b)) => {
				TileCover::from(t.intersection_bbox(b)?)
			}
			(TileCover::Tree(t1), TileCover::Tree(t2)) => TileCover::from(t1.intersection_tree(t2)?),
		})
	}

	/// Returns a new cover containing only the tiles shared by `self` and the
	/// corresponding level of `pyramid`.
	#[must_use]
	pub fn intersection_pyramid(&self, pyramid: &TilePyramid) -> Self {
		self
			.intersection_cover(pyramid.level_ref(self.level()))
			.expect("same-level operation")
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	/// `intersects_bbox` is consistent across the Bbox and Tree variants.
	/// Cover is built from bbox(4, 0,0,7,7) in both representations.
	#[rstest::rstest]
	#[case::overlaps(bbox(4, 5, 5, 10, 10), true)]
	#[case::corner_tile_at_x_max(bbox(4, 7, 7, 7, 7), true)]
	#[case::opposite_corner(bbox(4, 0, 0, 0, 0), true)]
	#[case::fully_outside(bbox(4, 10, 10, 15, 15), false)]
	#[case::edge_but_not_overlap(bbox(4, 8, 0, 15, 7), false)] // 8 > 7
	#[case::empty_never_overlaps(TileBBox::new_empty(4).unwrap(), false)]
	fn intersects_bbox_cases(#[case] other: TileBBox, #[case] expected: bool) {
		for cov in [
			TileCover::from(bbox(4, 0, 0, 7, 7)),
			TileCover::from(TileQuadtree::from_bbox(&bbox(4, 0, 0, 7, 7))),
		] {
			assert_eq!(cov.intersects_bbox(&other), expected);
		}
	}

	/// In-place `intersect_bbox` shrinks to the overlap or becomes empty when
	/// disjoint.
	#[rstest::rstest]
	#[case::overlap(bbox(4, 4, 4, 11, 11), Some(bbox(4, 4, 4, 7, 7)))]
	#[case::disjoint(bbox(4, 10, 10, 15, 15), None)]
	fn intersect_bbox_cases(#[case] clip: TileBBox, #[case] expected: Option<TileBBox>) {
		let mut c = TileCover::from(bbox(4, 0, 0, 7, 7));
		c.intersect_bbox(&clip).unwrap();
		match expected {
			Some(e) => assert_eq!(c.to_bbox(), e),
			None => assert!(c.is_empty()),
		}
	}

	#[test]
	fn intersect_bbox_zoom_mismatch_errors() {
		let mut c = TileCover::from(bbox(4, 0, 0, 7, 7));
		assert!(c.intersect_bbox(&bbox(5, 0, 0, 7, 7)).is_err());
	}

	#[test]
	fn intersection_bbox_is_pure() {
		let orig = TileCover::from(bbox(4, 0, 0, 7, 7));
		let out = orig.intersection_bbox(&bbox(4, 4, 4, 11, 11)).unwrap();
		assert_eq!(orig.count_tiles(), 64, "original unchanged");
		assert_eq!(out.to_bbox(), bbox(4, 4, 4, 7, 7));
	}

	/// `intersection_cover` yields the same result for all 4 combinations of
	/// Bbox / Tree variants on either side.
	#[rstest::rstest]
	#[case::bbox_bbox(false, false)]
	#[case::bbox_tree(false, true)]
	#[case::tree_bbox(true, false)]
	#[case::tree_tree(true, true)]
	fn intersection_cover_across_variants(#[case] a_is_tree: bool, #[case] b_is_tree: bool) {
		let a_bbox = bbox(4, 0, 0, 7, 7);
		let b_bbox = bbox(4, 4, 4, 11, 11);
		let a = if a_is_tree {
			TileCover::from(TileQuadtree::from_bbox(&a_bbox))
		} else {
			TileCover::from(a_bbox)
		};
		let b = if b_is_tree {
			TileCover::from(TileQuadtree::from_bbox(&b_bbox))
		} else {
			TileCover::from(b_bbox)
		};
		assert_eq!(a.intersection_cover(&b).unwrap().to_bbox(), bbox(4, 4, 4, 7, 7));
	}

	/// `intersects_tree` must agree with `intersects_bbox` on both variants.
	#[rstest::rstest]
	#[case::overlap(bbox(4, 4, 4, 11, 11), true)]
	#[case::disjoint(bbox(4, 10, 10, 15, 15), false)]
	fn intersects_tree_cases(#[case] other: TileBBox, #[case] expected: bool) {
		let other_tree = TileQuadtree::from_bbox(&other);
		for cov in [
			TileCover::from(bbox(4, 0, 0, 7, 7)),
			TileCover::from(TileQuadtree::from_bbox(&bbox(4, 0, 0, 7, 7))),
		] {
			assert_eq!(cov.intersects_tree(&other_tree), expected);
		}
	}

	/// `intersects_cover` over all four variant combinations.
	#[rstest::rstest]
	#[case::bbox_bbox(false, false, true)]
	#[case::bbox_tree(false, true, true)]
	#[case::tree_bbox(true, false, true)]
	#[case::tree_tree(true, true, true)]
	#[case::tree_tree_disjoint(true, true, false)]
	fn intersects_cover_across_variants(#[case] a_is_tree: bool, #[case] b_is_tree: bool, #[case] expect_overlap: bool) {
		let a_bbox = bbox(4, 0, 0, 7, 7);
		let b_bbox = if expect_overlap {
			bbox(4, 4, 4, 11, 11)
		} else {
			bbox(4, 10, 10, 15, 15)
		};
		let make = |is_tree: bool, b: TileBBox| -> TileCover {
			if is_tree {
				TileCover::from(TileQuadtree::from_bbox(&b))
			} else {
				TileCover::from(b)
			}
		};
		let a = make(a_is_tree, a_bbox);
		let b = make(b_is_tree, b_bbox);
		assert_eq!(a.intersects_cover(&b), expect_overlap);
	}

	#[test]
	fn intersects_pyramid_uses_matching_level() {
		let mut pyramid = TilePyramid::new_empty();
		pyramid.insert_bbox(&bbox(4, 0, 0, 7, 7)).unwrap();
		let cover_overlap = TileCover::from(bbox(4, 4, 4, 11, 11));
		let cover_disjoint = TileCover::from(bbox(4, 10, 10, 15, 15));
		assert!(cover_overlap.intersects_pyramid(&pyramid));
		assert!(!cover_disjoint.intersects_pyramid(&pyramid));
	}

	/// In-place `intersect_tree` on both variants shrinks to the overlap.
	#[rstest::rstest]
	#[case::from_bbox(false)]
	#[case::from_tree(true)]
	fn intersect_tree_cases(#[case] is_tree: bool) {
		let clip = TileQuadtree::from_bbox(&bbox(4, 4, 4, 11, 11));
		let mut c = if is_tree {
			TileCover::from(TileQuadtree::from_bbox(&bbox(4, 0, 0, 7, 7)))
		} else {
			TileCover::from(bbox(4, 0, 0, 7, 7))
		};
		c.intersect_tree(&clip).unwrap();
		assert_eq!(c.to_bbox(), bbox(4, 4, 4, 7, 7));
	}

	/// In-place `intersect_cover` across all four variant combinations.
	#[rstest::rstest]
	#[case::bbox_bbox(false, false)]
	#[case::bbox_tree(false, true)]
	#[case::tree_bbox(true, false)]
	#[case::tree_tree(true, true)]
	fn intersect_cover_across_variants(#[case] a_is_tree: bool, #[case] b_is_tree: bool) {
		let a_bbox = bbox(4, 0, 0, 7, 7);
		let b_bbox = bbox(4, 4, 4, 11, 11);
		let make = |is_tree: bool, b: TileBBox| -> TileCover {
			if is_tree {
				TileCover::from(TileQuadtree::from_bbox(&b))
			} else {
				TileCover::from(b)
			}
		};
		let mut a = make(a_is_tree, a_bbox);
		let b = make(b_is_tree, b_bbox);
		a.intersect_cover(&b).unwrap();
		assert_eq!(a.to_bbox(), bbox(4, 4, 4, 7, 7));
	}

	#[test]
	fn intersect_pyramid_narrows_to_level_overlap() {
		let mut pyramid = TilePyramid::new_empty();
		pyramid.insert_bbox(&bbox(4, 4, 4, 11, 11)).unwrap();
		let mut cover = TileCover::from(bbox(4, 0, 0, 7, 7));
		cover.intersect_pyramid(&pyramid);
		assert_eq!(cover.to_bbox(), bbox(4, 4, 4, 7, 7));
	}

	/// `intersection_tree` is pure on both variants.
	#[rstest::rstest]
	#[case::from_bbox(false)]
	#[case::from_tree(true)]
	fn intersection_tree_cases(#[case] is_tree: bool) {
		let clip = TileQuadtree::from_bbox(&bbox(4, 4, 4, 11, 11));
		let orig = if is_tree {
			TileCover::from(TileQuadtree::from_bbox(&bbox(4, 0, 0, 7, 7)))
		} else {
			TileCover::from(bbox(4, 0, 0, 7, 7))
		};
		let out = orig.intersection_tree(&clip).unwrap();
		assert_eq!(orig.count_tiles(), 64, "original unchanged");
		assert_eq!(out.to_bbox(), bbox(4, 4, 4, 7, 7));
	}

	#[test]
	fn intersection_pyramid_returns_overlap() {
		let mut pyramid = TilePyramid::new_empty();
		pyramid.insert_bbox(&bbox(4, 4, 4, 11, 11)).unwrap();
		let cover = TileCover::from(bbox(4, 0, 0, 7, 7));
		let out = cover.intersection_pyramid(&pyramid);
		assert_eq!(cover.count_tiles(), 64, "original unchanged");
		assert_eq!(out.to_bbox(), bbox(4, 4, 4, 7, 7));
	}
}
