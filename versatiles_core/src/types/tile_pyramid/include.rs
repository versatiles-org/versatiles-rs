use crate::{TileBBox, TileCoord, TileCover, TilePyramid, TileQuadtree};

impl TilePyramid {
	/// Returns `true` if `coord` is covered at its zoom level in this pyramid.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		self.level_ref(coord.level).includes_coord(coord)
	}

	/// Returns `true` if every tile in `bbox` is covered at `bbox`'s zoom level.
	///
	/// An empty `bbox` is vacuously included (returns `true`).
	#[must_use]
	pub fn includes_bbox(&self, bbox: &TileBBox) -> bool {
		self.level_ref(bbox.level()).includes_bbox(bbox)
	}

	/// Returns `true` if every tile in `tree` is covered at `tree`'s zoom level.
	#[must_use]
	pub fn includes_tree(&self, tree: &TileQuadtree) -> bool {
		self.level_ref(tree.level()).includes_tree(tree)
	}

	/// Returns `true` if every tile in `cover` is covered at `cover`'s zoom level.
	#[must_use]
	pub fn includes_cover(&self, cover: &TileCover) -> bool {
		self.level_ref(cover.level()).includes_cover(cover)
	}

	/// Returns `true` if for every zoom level, every tile in `pyramid` is also
	/// in `self`.
	///
	/// An empty `pyramid` is vacuously included (returns `true`).
	#[must_use]
	pub fn includes_pyramid(&self, pyramid: &TilePyramid) -> bool {
		self.iter().zip(pyramid.iter()).all(|(a, b)| a.includes_cover(b))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}
	fn coord(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	fn pyramid_from(b: TileBBox) -> TilePyramid {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&b).unwrap();
		p
	}

	/// Pyramid with bbox(5, 3,4,10,15) at z=5. Test coord inclusion.
	#[rstest]
	#[case::inside(coord(5, 5, 7), true)]
	#[case::outside_at_same_level(coord(5, 0, 0), false)]
	#[case::other_level_is_empty(coord(6, 5, 7), false)]
	fn includes_coord_cases(#[case] c: TileCoord, #[case] expected: bool) {
		assert_eq!(pyramid_from(bbox(5, 3, 4, 10, 15)).includes_coord(&c), expected);
	}

	/// Pyramid with bbox(5, 0,0,15,15) at z=5. Test bbox inclusion.
	#[rstest]
	#[case::subset(bbox(5, 2, 2, 8, 8), true)]
	#[case::extends_beyond(bbox(5, 0, 0, 20, 20), false)]
	fn includes_bbox_cases(#[case] query: TileBBox, #[case] expected: bool) {
		assert_eq!(pyramid_from(bbox(5, 0, 0, 15, 15)).includes_bbox(&query), expected);
	}

	/// `a.includes_pyramid(b)` — various (a, b) pairs.
	#[rstest]
	#[case::superset(pyramid_from(bbox(5, 0, 0, 15, 15)), pyramid_from(bbox(5, 2, 2, 8, 8)), true)]
	#[case::not_superset(pyramid_from(bbox(5, 2, 2, 8, 8)), pyramid_from(bbox(5, 0, 0, 15, 15)), false)]
	#[case::any_includes_empty(TilePyramid::new_full_up_to(5), TilePyramid::new_empty(), true)]
	fn includes_pyramid_cases(#[case] a: TilePyramid, #[case] b: TilePyramid, #[case] expected: bool) {
		assert_eq!(a.includes_pyramid(&b), expected);
	}

	/// Pyramid with bbox(5, 0,0,15,15) at z=5. Test tree inclusion.
	#[rstest]
	#[case::subset_tree(TileQuadtree::from_bbox(&bbox(5, 2, 2, 8, 8)), true)]
	#[case::tree_extends_beyond(TileQuadtree::from_bbox(&bbox(5, 10, 10, 20, 20)), false)]
	fn includes_tree_cases(#[case] tree: TileQuadtree, #[case] expected: bool) {
		assert_eq!(pyramid_from(bbox(5, 0, 0, 15, 15)).includes_tree(&tree), expected);
	}

	/// Pyramid with bbox(5, 0,0,15,15) at z=5. Test cover inclusion across variants.
	#[rstest]
	#[case::subset_bbox_cover(TileCover::from(bbox(5, 2, 2, 8, 8)), true)]
	#[case::subset_tree_cover(TileCover::from(TileQuadtree::from_bbox(&bbox(5, 2, 2, 8, 8))), true)]
	#[case::cover_extends_beyond(TileCover::from(bbox(5, 10, 10, 20, 20)), false)]
	fn includes_cover_cases(#[case] cover: TileCover, #[case] expected: bool) {
		assert_eq!(pyramid_from(bbox(5, 0, 0, 15, 15)).includes_cover(&cover), expected);
	}
}
