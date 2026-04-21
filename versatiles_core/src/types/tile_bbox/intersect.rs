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
		self.x_min().unwrap() <= bbox.x_max().unwrap()
			&& self.x_max().unwrap() >= bbox.x_min().unwrap()
			&& self.y_min().unwrap() <= bbox.y_max().unwrap()
			&& self.y_max().unwrap() >= bbox.y_min().unwrap()
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
		self.intersect_cover(pyramid.level_ref(self.level)).unwrap();
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
		self.intersection_cover(pyramid.level_ref(self.level)).unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	fn bb(level: u8, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x_min, y_min, x_max, y_max).unwrap()
	}

	// ------------------------------ intersect_with / intersect_pyramid ------------------------------
	#[rstest]
	#[case(bb(5, 10,10, 20,20), bb(5, 15,15, 25,25), [15,15,20,20])] // partial overlap
	#[case(bb(5, 10,10, 20,20), bb(5, 0,0, 5,5),     [0,0,0,0])] // no overlap → empty
	#[case(bb(5, 10,10, 20,20), bb(5, 10,10, 20,20), [10,10,20,20])] // identical
	fn intersect_cases(#[case] mut a: TileBBox, #[case] b: TileBBox, #[case] exp: [u32; 4]) {
		a.intersect_bbox(&b).unwrap();
		if exp == [0, 0, 0, 0] && (a.width() == 0 || a.height() == 0) {
			// empty expected; nothing more to assert
			return;
		}
		assert_eq!(a.to_array().unwrap(), exp);
	}

	#[test]
	fn intersect_pyramid_shrinks() {
		use crate::TilePyramid;
		let full = TileBBox::new_full(5).unwrap();
		let pyramid = TilePyramid::from([full].as_slice());
		let mut b = bb(5, 12, 12, 20, 20);
		b.intersect_pyramid(&pyramid);
		assert_eq!(b, bb(5, 12, 12, 20, 20)); // full pyramid covers all

		let small = bb(5, 14, 15, 16, 18);
		let py_small = TilePyramid::from([small].as_slice());
		let mut b2 = bb(5, 12, 12, 20, 20);
		b2.intersect_pyramid(&py_small);
		assert_eq!(b2, small);
	}

	#[test]
	fn boolean_operations() -> anyhow::Result<()> {
		let bbox1 = TileBBox::from_min_and_max(4, 0, 11, 2, 13)?;
		let bbox2 = TileBBox::from_min_and_max(4, 1, 10, 3, 12)?;

		let mut bbox1_intersect = bbox1;
		bbox1_intersect.intersect_bbox(&bbox2)?;
		assert_eq!(bbox1_intersect, TileBBox::from_min_and_max(4, 1, 11, 2, 12)?);

		let mut bbox1_union = bbox1;
		bbox1_union.insert_bbox(&bbox2)?;
		assert_eq!(bbox1_union, TileBBox::from_min_and_max(4, 0, 10, 3, 13)?);

		Ok(())
	}

	#[test]
	fn test_intersect_bbox() -> anyhow::Result<()> {
		let mut bbox1 = TileBBox::from_min_and_max(4, 0, 11, 2, 13)?;
		let bbox2 = TileBBox::from_min_and_max(4, 1, 10, 3, 12)?;
		bbox1.intersect_bbox(&bbox2)?;
		assert_eq!(bbox1, TileBBox::from_min_and_max(4, 1, 11, 2, 12)?);
		Ok(())
	}

	#[test]
	fn test_overlaps_bbox() -> anyhow::Result<()> {
		let bbox1 = TileBBox::from_min_and_max(4, 0, 11, 2, 13)?;
		let bbox2 = TileBBox::from_min_and_max(4, 1, 10, 3, 12)?;
		assert!(bbox1.intersects_bbox(&bbox2));

		let bbox3 = TileBBox::from_min_and_max(4, 8, 8, 9, 9)?;
		assert!(!bbox1.intersects_bbox(&bbox3));

		Ok(())
	}

	#[test]
	fn should_intersect_bboxes_correctly_and_handle_empty_and_different_levels() -> anyhow::Result<()> {
		let mut bbox1 = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::from_min_and_max(6, 10, 15, 20, 25)?;
		let bbox3 = TileBBox::from_min_and_max(6, 16, 21, 20, 25)?;

		bbox1.intersect_bbox(&bbox2)?;
		assert_eq!(bbox1, TileBBox::from_min_and_max(6, 10, 15, 15, 20)?);

		// Intersect with a non-overlapping bounding box
		bbox1.intersect_bbox(&bbox3)?;
		assert!(bbox1.is_empty());

		// Attempting to intersect with a bounding box of different zoom level
		let bbox_diff_level = TileBBox::from_min_and_max(5, 10, 15, 15, 20)?;
		let result = bbox1.intersect_bbox(&bbox_diff_level);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_correctly_determine_bbox_overlap() -> anyhow::Result<()> {
		let bbox1 = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::from_min_and_max(6, 10, 15, 20, 25)?;
		let bbox3 = TileBBox::from_min_and_max(6, 16, 21, 20, 25)?;

		assert!(bbox1.intersects_bbox(&bbox2));
		assert!(!bbox1.intersects_bbox(&bbox3));
		assert!(bbox1.intersects_bbox(&bbox1));
		assert!(bbox1.intersects_bbox(&bbox1.clone()));

		Ok(())
	}

	#[test]
	fn should_handle_bbox_overlap_edge_cases() -> anyhow::Result<()> {
		let bbox1 = TileBBox::from_min_and_max(4, 0, 0, 5, 5)?;
		let bbox2 = TileBBox::from_min_and_max(4, 5, 5, 10, 10)?;
		let bbox3 = TileBBox::from_min_and_max(4, 6, 6, 10, 10)?;
		let bbox4 = TileBBox::from_min_and_max(4, 0, 0, 5, 5)?;

		// Overlapping at the edge
		assert!(bbox1.intersects_bbox(&bbox2));

		// No overlapping
		assert!(!bbox1.intersects_bbox(&bbox3));

		// Completely overlapping
		assert!(bbox1.intersects_bbox(&bbox4));

		// One empty bounding box
		let empty_bbox = TileBBox::new_empty(4)?;
		assert!(!bbox1.intersects_bbox(&empty_bbox));

		Ok(())
	}

	#[test]
	fn test_intersect_pyramid() -> anyhow::Result<()> {
		use crate::TilePyramid;
		// Create a pyramid with a known full bbox at level 5
		let pyramid = TilePyramid::from([TileBBox::new_full(5)?].as_slice());

		// Create a bbox partially overlapping the full bbox
		let mut bbox = TileBBox::from_min_and_max(5, 10, 10, 20, 20)?;
		bbox.intersect_pyramid(&pyramid);

		// Since the pyramid covers the full range, intersection should not modify bbox
		assert_eq!(bbox, TileBBox::from_min_and_max(5, 10, 10, 20, 20)?);

		// Now create a pyramid with a smaller bbox (subset)
		let smaller_bbox = TileBBox::from_min_and_max(5, 12, 12, 18, 18)?;
		let pyramid_small = TilePyramid::from([smaller_bbox].as_slice());
		let mut bbox = TileBBox::from_min_and_max(5, 10, 10, 20, 20)?;
		bbox.intersect_pyramid(&pyramid_small);

		// Intersection should shrink to overlap region
		assert_eq!(bbox, TileBBox::from_min_and_max(5, 12, 12, 18, 18)?);

		Ok(())
	}
}
