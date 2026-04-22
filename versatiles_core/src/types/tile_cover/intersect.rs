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
		self.intersect_cover(pyramid.level_ref(self.level())).unwrap();
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
		self.intersection_cover(pyramid.level_ref(self.level())).unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn intersects_bbox() {
		let c = TileCover::from(bbox(4, 0, 0, 7, 7));
		assert!(c.intersects_bbox(&bbox(4, 5, 5, 10, 10)));
		assert!(!c.intersects_bbox(&bbox(4, 10, 10, 15, 15)));
	}

	#[test]
	fn intersects_bbox_tree_variant() {
		let c = TileCover::from(TileQuadtree::from_bbox(&bbox(4, 0, 0, 7, 7)));
		assert!(c.intersects_bbox(&bbox(4, 5, 5, 10, 10)));
		assert!(!c.intersects_bbox(&bbox(4, 10, 10, 15, 15)));
	}

	// ── Parameterized intersects_bbox across both variants ──────────────────
	fn variants(cov: TileBBox) -> Vec<TileCover> {
		vec![TileCover::from(cov), TileCover::from(TileQuadtree::from_bbox(&cov))]
	}

	#[rstest::rstest]
	#[case(bbox(4, 5, 5, 10, 10), true)] // overlaps
	#[case(bbox(4, 7, 7, 7, 7), true)] // corner tile (self.x_max)
	#[case(bbox(4, 0, 0, 0, 0), true)] // opposite corner
	#[case(bbox(4, 10, 10, 15, 15), false)] // fully outside
	#[case(bbox(4, 8, 0, 15, 7), false)] // touches edge but not overlap (8 > 7)
	#[case(TileBBox::new_empty(4).unwrap(), false)] // empty never intersects
	fn intersects_bbox_cases(#[case] other: TileBBox, #[case] expected: bool) {
		for cov in variants(bbox(4, 0, 0, 7, 7)) {
			assert_eq!(cov.intersects_bbox(&other), expected);
		}
	}

	#[test]
	fn intersect_bbox_shrinks_and_clears() {
		let mut c = TileCover::from(bbox(4, 0, 0, 7, 7));
		c.intersect_bbox(&bbox(4, 4, 4, 11, 11)).unwrap();
		assert_eq!(c.to_bbox(), bbox(4, 4, 4, 7, 7));

		// Intersect with disjoint → empty.
		let mut c = TileCover::from(bbox(4, 0, 0, 7, 7));
		c.intersect_bbox(&bbox(4, 10, 10, 15, 15)).unwrap();
		assert!(c.is_empty());
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
		// Original is unchanged.
		assert_eq!(orig.count_tiles(), 64);
		assert_eq!(out.to_bbox(), bbox(4, 4, 4, 7, 7));
	}

	#[test]
	fn intersection_cover_across_variants() {
		let a_b = TileCover::from(bbox(4, 0, 0, 7, 7));
		let a_t = TileCover::from(TileQuadtree::from_bbox(&bbox(4, 0, 0, 7, 7)));
		let b_b = TileCover::from(bbox(4, 4, 4, 11, 11));
		let b_t = TileCover::from(TileQuadtree::from_bbox(&bbox(4, 4, 4, 11, 11)));
		// All 4 combinations should produce the same coverage.
		let expected = bbox(4, 4, 4, 7, 7);
		for a in [&a_b, &a_t] {
			for b in [&b_b, &b_t] {
				let i = a.intersection_cover(b).unwrap();
				assert_eq!(i.to_bbox(), expected, "variants {a:?} ∩ {b:?}");
			}
		}
	}
}
