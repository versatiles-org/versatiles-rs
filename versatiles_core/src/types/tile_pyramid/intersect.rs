use crate::{TileBBox, TileCover, TilePyramid, TileQuadtree};

impl TilePyramid {
	/// Returns `true` if the level matching `bbox`'s zoom shares at least one
	/// tile with `bbox`.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		self.level_ref(bbox.level()).intersects_bbox(bbox)
	}

	/// Returns `true` if the level matching `tree`'s zoom shares at least one
	/// tile with `tree`.
	#[must_use]
	pub fn intersects_tree(&self, tree: &TileQuadtree) -> bool {
		self.level_ref(tree.level()).intersects_tree(tree)
	}

	/// Returns `true` if the level matching `cover`'s zoom shares at least one
	/// tile with `cover`.
	#[must_use]
	pub fn intersects_cover(&self, cover: &TileCover) -> bool {
		self.level_ref(cover.level()).intersects_cover(cover)
	}

	/// Returns `true` if any zoom level of `self` shares at least one tile with
	/// the same level of `pyramid`.
	#[must_use]
	pub fn intersects_pyramid(&self, pyramid: &TilePyramid) -> bool {
		self.iter().zip(pyramid.iter()).any(|(a, b)| a.intersects_cover(b))
	}

	/// Shrinks each level of `self` in place to the tiles also present in the
	/// corresponding level of `pyramid`.
	pub fn intersect_pyramid(&mut self, pyramid: &TilePyramid) {
		self
			.levels
			.iter_mut()
			.zip(pyramid.levels.iter())
			.for_each(|(a, b)| a.intersect_cover(b).expect("same-level operation"));
	}

	/// Returns a new pyramid where each level contains only the tiles shared by
	/// `self` and the corresponding level of `pyramid`.
	#[must_use]
	pub fn intersection_pyramid(&self, pyramid: &TilePyramid) -> Self {
		let mut result = self.clone();
		result.intersect_pyramid(pyramid);
		result
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{GeoBBox, TileCoord};
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

	/// Pyramid with bbox(4, 0,0,7,7) intersecting different query bboxes.
	#[rstest]
	#[case::overlap(bbox(4, 5, 5, 10, 10), true)]
	#[case::disjoint(bbox(4, 10, 10, 15, 15), false)]
	fn intersects_bbox_cases(#[case] query: TileBBox, #[case] expected: bool) {
		assert_eq!(pyramid_from(bbox(4, 0, 0, 7, 7)).intersects_bbox(&query), expected);
	}

	/// `a.intersects_pyramid(b)` — subset / disjoint.
	#[rstest]
	#[case::subset(pyramid_from(bbox(5, 2, 2, 8, 8)), true)]
	#[case::disjoint(pyramid_from(bbox(5, 20, 20, 25, 25)), false)]
	fn intersects_pyramid_cases(#[case] b: TilePyramid, #[case] expected: bool) {
		let a = pyramid_from(bbox(5, 0, 0, 15, 15));
		assert_eq!(a.intersects_pyramid(&b), expected);
	}

	#[test]
	fn intersect_pyramid_narrows_to_overlap() {
		let mut a = pyramid_from(bbox(5, 0, 0, 15, 15));
		a.intersect_pyramid(&pyramid_from(bbox(5, 10, 10, 25, 25)));
		assert!(a.includes_coord(&coord(5, 12, 12)));
		assert!(!a.includes_coord(&coord(5, 2, 2)));
	}

	#[test]
	fn intersect_geo_bbox_restricts_every_level() {
		let mut p = TilePyramid::new_full();
		p.intersect_geo_bbox(&GeoBBox::new(10.0, 50.0, 15.0, 55.0).unwrap())
			.unwrap();
		assert!(!p.is_empty());
		assert_eq!(p.level_ref(10).count_tiles(), 375);
	}

	#[test]
	fn intersection_bbox_via_level_ref() {
		let p = pyramid_from(bbox(4, 0, 0, 7, 7));
		let out = p.level_ref(4).intersection_bbox(&bbox(4, 4, 4, 11, 11)).unwrap();
		assert_eq!(out.to_bbox(), bbox(4, 4, 4, 7, 7));
	}
}
