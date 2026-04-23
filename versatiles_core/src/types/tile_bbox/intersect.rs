use anyhow::Result;

use crate::{TileBBox, TileCover, TilePyramid, TileQuadtree, types::info_trait::TileCoverInfo};

impl TileBBox {
	/// Returns `true` if `self` and `bbox` share at least one tile.
	///
	/// Returns `false` if either bbox is empty or the zoom levels differ.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		if self.level != bbox.level || self.is_empty() || bbox.is_empty() {
			return false;
		}

		// Safety: is_empty() checked above; getters are valid.
		self.x_min().expect("bbox is non-empty") <= bbox.x_max().expect("bbox is non-empty")
			&& self.x_max().expect("bbox is non-empty") >= bbox.x_min().expect("bbox is non-empty")
			&& self.y_min().expect("bbox is non-empty") <= bbox.y_max().expect("bbox is non-empty")
			&& self.y_max().expect("bbox is non-empty") >= bbox.y_min().expect("bbox is non-empty")
	}

	/// Returns `true` if `self` and `tree` share at least one tile.
	#[must_use]
	pub fn intersects_tree(&self, tree: &TileQuadtree) -> bool {
		tree.intersects_bbox(self)
	}

	/// Returns `true` if `self` and `cover` share at least one tile.
	#[must_use]
	pub fn intersects_cover(&self, cover: &TileCover) -> bool {
		cover.intersects_bbox(self)
	}

	/// Returns `true` if `self` shares at least one tile with the corresponding
	/// level of `pyramid`.
	#[must_use]
	pub fn intersects_pyramid(&self, pyramid: &TilePyramid) -> bool {
		self.intersects_cover(pyramid.level_ref(self.level))
	}

	/// Shrinks `self` in place to the tiles shared with `bbox` (set intersection).
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		self.ensure_same_level(bbox, "intersect")?;

		if self.is_empty() {
			return Ok(()); // empty ∩ anything = empty
		}

		if bbox.is_empty() {
			self.set_empty();
			return Ok(()); // anything ∩ empty = empty
		}

		let x_min = self.x_min()?.max(bbox.x_min()?);
		let y_min = self.y_min()?.max(bbox.y_min()?);
		let x_max = self.x_max()?.min(bbox.x_max()?);
		let y_max = self.y_max()?.min(bbox.y_max()?);

		if x_min > x_max || y_min > y_max {
			self.set_empty();
		} else {
			self.set_min_and_max(x_min, y_min, x_max, y_max)?;
		}

		Ok(())
	}

	/// Shrinks `self` in place to the tiles shared with `tree`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_tree(&mut self, tree: &TileQuadtree) -> Result<()> {
		self.ensure_same_level(tree, "intersect")?;
		*self = tree.intersection_bbox(self)?.to_bbox();
		Ok(())
	}

	/// Shrinks `self` in place to the tiles shared with `cover`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_cover(&mut self, cover: &TileCover) -> Result<()> {
		self.ensure_same_level(cover, "intersect")?;
		*self = cover.intersection_bbox(self)?.to_bbox();
		Ok(())
	}

	/// Shrinks `self` in place to the tiles shared with the corresponding level
	/// of `pyramid`.
	pub fn intersect_pyramid(&mut self, pyramid: &TilePyramid) {
		self
			.intersect_cover(pyramid.level_ref(self.level))
			.expect("same-level operation");
	}

	/// Returns a new bbox containing only the tiles shared by `self` and `bbox`.
	///
	/// Returns an empty bbox if they do not overlap.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_bbox(&self, bbox: &TileBBox) -> Result<Self> {
		self.ensure_same_level(bbox, "intersect")?;

		if self.is_empty() || bbox.is_empty() {
			return TileBBox::new_empty(self.level);
		}

		let x_min = self.x_min()?.max(bbox.x_min()?);
		let y_min = self.y_min()?.max(bbox.y_min()?);
		let x_max = self.x_max()?.min(bbox.x_max()?);
		let y_max = self.y_max()?.min(bbox.y_max()?);

		if x_min > x_max || y_min > y_max {
			return TileBBox::new_empty(self.level);
		}
		TileBBox::from_min_and_max(self.level, x_min, y_min, x_max, y_max)
	}

	/// Returns a new bbox containing only the tiles shared by `self` and `tree`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_tree(&self, tree: &TileQuadtree) -> Result<Self> {
		self.ensure_same_level(tree, "intersect")?;
		Ok(tree.intersection_bbox(self)?.to_bbox())
	}

	/// Returns a new bbox containing only the tiles shared by `self` and `cover`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_cover(&self, cover: &TileCover) -> Result<Self> {
		self.ensure_same_level(cover, "intersect")?;
		Ok(cover.intersection_bbox(self)?.to_bbox())
	}

	/// Returns a new bbox containing only the tiles shared by `self` and the
	/// corresponding level of `pyramid`.
	#[must_use]
	pub fn intersection_pyramid(&self, pyramid: &TilePyramid) -> Self {
		self
			.intersection_cover(pyramid.level_ref(self.level))
			.expect("same-level operation")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	fn bb(level: u8, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x_min, y_min, x_max, y_max).unwrap()
	}

	/// In-place `intersect_bbox(a, b)` → resulting bbox (None means empty).
	#[rstest]
	#[case::partial(bb(6, 5, 10, 15, 20), bb(6, 10, 15, 20, 25), Some(bb(6, 10, 15, 15, 20)))]
	#[case::edge_at_corner(bb(4, 0, 11, 2, 13), bb(4, 1, 10, 3, 12), Some(bb(4, 1, 11, 2, 12)))]
	#[case::disjoint(bb(6, 5, 10, 15, 20), bb(6, 16, 21, 20, 25), None)]
	#[case::identical(bb(5, 10, 10, 20, 20), bb(5, 10, 10, 20, 20), Some(bb(5, 10, 10, 20, 20)))]
	fn intersect_bbox_cases(#[case] mut a: TileBBox, #[case] b: TileBBox, #[case] expected: Option<TileBBox>) {
		a.intersect_bbox(&b).unwrap();
		match expected {
			Some(e) => assert_eq!(a, e),
			None => assert!(a.is_empty()),
		}
	}

	#[test]
	fn intersect_bbox_zoom_mismatch_errors() {
		let mut a = bb(6, 5, 10, 15, 20);
		assert!(a.intersect_bbox(&bb(5, 10, 15, 15, 20)).is_err());
	}

	/// `a.intersects_bbox(b)` — overlap / edge / disjoint / empty.
	#[rstest]
	#[case::partial(bb(6, 5, 10, 15, 20), bb(6, 10, 15, 20, 25), true)]
	#[case::edge(bb(4, 0, 0, 5, 5), bb(4, 5, 5, 10, 10), true)]
	#[case::just_beyond_edge(bb(4, 0, 0, 5, 5), bb(4, 6, 6, 10, 10), false)]
	#[case::disjoint(bb(6, 5, 10, 15, 20), bb(6, 16, 21, 20, 25), false)]
	#[case::self_overlap(bb(6, 5, 10, 15, 20), bb(6, 5, 10, 15, 20), true)]
	#[case::empty_never_overlaps(bb(4, 0, 0, 5, 5), TileBBox::new_empty(4).unwrap(), false)]
	fn intersects_bbox_cases(#[case] a: TileBBox, #[case] b: TileBBox, #[case] expected: bool) {
		assert_eq!(a.intersects_bbox(&b), expected);
	}

	#[test]
	fn insert_bbox_is_bounding_union() -> anyhow::Result<()> {
		let mut a = bb(4, 0, 11, 2, 13);
		a.insert_bbox(&bb(4, 1, 10, 3, 12))?;
		assert_eq!(a, bb(4, 0, 10, 3, 13));
		Ok(())
	}

	/// `intersect_pyramid` — pyramid restricts the bbox at self.level.
	#[rstest]
	#[case::full_pyramid_is_noop(
		TileBBox::new_full(5).unwrap(),
		bb(5, 12, 12, 20, 20),
		bb(5, 12, 12, 20, 20),
	)]
	#[case::pyramid_subset(bb(5, 12, 12, 18, 18), bb(5, 10, 10, 20, 20), bb(5, 12, 12, 18, 18))]
	#[case::small_pyramid_drops_to_inner(bb(5, 14, 15, 16, 18), bb(5, 12, 12, 20, 20), bb(5, 14, 15, 16, 18))]
	fn intersect_pyramid_cases(
		#[case] pyramid_bbox: TileBBox,
		#[case] mut target: TileBBox,
		#[case] expected: TileBBox,
	) {
		use crate::TilePyramid;
		let pyramid = TilePyramid::from([pyramid_bbox].as_slice());
		target.intersect_pyramid(&pyramid);
		assert_eq!(target, expected);
	}
}
