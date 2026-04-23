use crate::{TileBBox, TileCoord, TileCover, TileQuadtree};

impl TileCover {
	/// Returns `true` if `coord` is covered by `self`.
	///
	/// # Panics
	/// Panics if `coord` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		match self {
			TileCover::Bbox(b) => b.includes_coord(coord),
			TileCover::Tree(t) => t.includes_coord(coord),
		}
	}

	/// Returns `true` if every tile in `bbox` is also covered by `self`.
	///
	/// An empty `bbox` is vacuously included (returns `true`).
	///
	/// # Panics
	/// Panics if `bbox` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_bbox(&self, bbox: &TileBBox) -> bool {
		match self {
			TileCover::Bbox(b) => b.includes_bbox(bbox),
			TileCover::Tree(t) => t.includes_bbox(bbox),
		}
	}

	/// Returns `true` if every tile in `tree` is also covered by `self`.
	///
	/// # Panics
	/// Panics if `tree` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_tree(&self, tree: &TileQuadtree) -> bool {
		match self {
			TileCover::Bbox(b) => b.includes_tree(tree),
			TileCover::Tree(t) => t.includes_tree(tree),
		}
	}

	/// Returns `true` if every tile in `cover` is also covered by `self`.
	///
	/// # Panics
	/// Panics if `cover` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_cover(&self, cover: &TileCover) -> bool {
		match (self, cover) {
			(TileCover::Bbox(b1), TileCover::Bbox(b2)) => b1.includes_bbox(b2),
			(TileCover::Bbox(b), TileCover::Tree(t)) => b.includes_tree(t),
			(TileCover::Tree(t), TileCover::Bbox(b)) => t.includes_bbox(b),
			(TileCover::Tree(t1), TileCover::Tree(t2)) => t1.includes_tree(t2),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}
	fn coord(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	// ── Level-mismatch panics (tree and bbox variants panic with different
	//    messages, so each needs its own `should_panic` expectation). ───────

	#[test]
	#[should_panic(expected = "assertion `left == right` failed")]
	fn includes_coord_level_mismatch_tree_panics() {
		let c = TileCover::from(TileQuadtree::new_full(4).unwrap());
		let _ = c.includes_coord(&coord(5, 0, 0));
	}

	#[test]
	#[should_panic(expected = "Cannot compare TileBBox with level=")]
	fn includes_coord_level_mismatch_bbox_panics() {
		let c = TileCover::from(bbox(4, 0, 0, 15, 15));
		let _ = c.includes_coord(&coord(5, 0, 0));
	}

	#[test]
	#[should_panic(expected = "assertion `left == right` failed")]
	fn includes_bbox_level_mismatch_tree_panics() {
		let c = TileCover::from(TileQuadtree::new_full(4).unwrap());
		let _ = c.includes_bbox(&bbox(5, 0, 0, 15, 15));
	}

	#[test]
	#[should_panic(expected = "Cannot compare TileBBox with level=")]
	fn includes_bbox_level_mismatch_bbox_panics() {
		let c = TileCover::from(bbox(4, 0, 0, 15, 15));
		let _ = c.includes_bbox(&bbox(5, 0, 0, 15, 15));
	}

	// ── Parameterized positive/negative cases across both variants ───────────
	fn variants(cover_bbox: TileBBox) -> Vec<TileCover> {
		vec![
			TileCover::from(cover_bbox),
			TileCover::from(TileQuadtree::from_bbox(&cover_bbox)),
		]
	}

	#[rstest::rstest]
	#[case(coord(5, 5, 7), true)] // inside
	#[case(coord(5, 3, 4), true)] // corner min
	#[case(coord(5, 10, 15), true)] // corner max
	#[case(coord(5, 0, 0), false)] // outside min
	#[case(coord(5, 11, 15), false)] // just past x_max
	#[case(coord(5, 10, 16), false)] // just past y_max
	fn includes_coord_both_variants(#[case] c: TileCoord, #[case] expected: bool) {
		for cov in variants(bbox(5, 3, 4, 10, 15)) {
			assert_eq!(cov.includes_coord(&c), expected, "{cov:?} vs {c:?}");
		}
	}

	#[rstest::rstest]
	#[case(bbox(5, 2, 2, 8, 8), true)] // strict subset
	#[case(bbox(5, 0, 0, 15, 15), true)] // equal
	#[case(bbox(5, 0, 0, 16, 16), false)] // extends past max
	#[case(TileBBox::new_empty(5).unwrap(), true)] // empty is subset of anything
	fn includes_bbox_both_variants(#[case] inner: TileBBox, #[case] expected: bool) {
		for cov in variants(bbox(5, 0, 0, 15, 15)) {
			assert_eq!(cov.includes_bbox(&inner), expected);
		}
	}

	/// Empty and full covers at a given level:
	///   - empty.includes_coord(any) → false
	///   - empty.includes_bbox(empty) → true (empty is subset of any set)
	///   - full.includes_coord(any in-range) → true
	///   - full.includes_bbox(any at same level) → true
	#[rstest::rstest]
	#[case::empty_contains_nothing_at_point(TileCover::new_empty(4).unwrap(), coord(4, 0, 0), false)]
	#[case::empty_contains_empty_bbox_trivially(TileCover::new_empty(4).unwrap(), coord(4, 0, 0), false)]
	#[case::full_contains_origin(TileCover::new_full(3).unwrap(), coord(3, 0, 0), true)]
	#[case::full_contains_last(TileCover::new_full(3).unwrap(), coord(3, 7, 7), true)]
	fn empty_and_full_cover_inclusion(#[case] c: TileCover, #[case] point: TileCoord, #[case] expected: bool) {
		assert_eq!(c.includes_coord(&point), expected);
	}

	#[test]
	fn empty_cover_includes_empty_bbox() {
		let empty = TileCover::new_empty(4).unwrap();
		assert!(empty.includes_bbox(&TileBBox::new_empty(4).unwrap()));
		assert!(!empty.includes_bbox(&bbox(4, 0, 0, 0, 0)));
	}

	#[test]
	fn full_cover_includes_everything_at_its_level() {
		let full = TileCover::new_full(3).unwrap();
		assert!(full.includes_bbox(&bbox(3, 0, 0, 7, 7)));
		assert!(full.includes_bbox(&TileBBox::new_empty(3).unwrap()));
	}

	/// `outer.includes_cover(inner)` across every combination of Bbox and
	/// Tree variants on both sides: outer=bbox(4, 0,0,15,15),
	/// inner=bbox(4, 3,3,10,10). Outer ⊇ inner in all 4 combinations; inner ⊉
	/// outer in any of them.
	#[rstest::rstest]
	#[case::bb_then_bb(false, false, true, false)]
	#[case::bb_then_tt(false, true, true, false)]
	#[case::tt_then_bb(true, false, true, false)]
	#[case::tt_then_tt(true, true, true, false)]
	fn includes_cover_across_variants(
		#[case] outer_is_tree: bool,
		#[case] inner_is_tree: bool,
		#[case] outer_includes_inner: bool,
		#[case] inner_includes_outer: bool,
	) {
		let outer_bbox = bbox(4, 0, 0, 15, 15);
		let inner_bbox = bbox(4, 3, 3, 10, 10);
		let outer = if outer_is_tree {
			TileCover::from(TileQuadtree::from_bbox(&outer_bbox))
		} else {
			TileCover::from(outer_bbox)
		};
		let inner = if inner_is_tree {
			TileCover::from(TileQuadtree::from_bbox(&inner_bbox))
		} else {
			TileCover::from(inner_bbox)
		};
		assert_eq!(outer.includes_cover(&inner), outer_includes_inner);
		assert_eq!(inner.includes_cover(&outer), inner_includes_outer);
	}
}
