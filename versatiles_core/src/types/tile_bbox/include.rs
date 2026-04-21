use crate::{TileBBox, TileCoord, TileCover, TileQuadtree};

impl TileBBox {
	/// Returns `true` if `coord` falls within this bbox.
	///
	/// # Panics
	/// Panics if `coord` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		assert_eq!(
			self.level, coord.level,
			"Cannot compare TileBBox with level={} with TileCoord with level={}",
			self.level, coord.level
		);
		if self.is_empty() {
			return false;
		}
		// Safety: is_empty() checked above; x_min/y_min/x_max/y_max are valid.
		coord.x >= self.x_min().unwrap()
			&& coord.x <= self.x_max().unwrap()
			&& coord.y >= self.y_min().unwrap()
			&& coord.y <= self.y_max().unwrap()
	}

	/// Returns `true` if every tile in `bbox` is also in `self`.
	///
	/// An empty `bbox` is a subset of any set, so this returns `true` when
	/// `bbox` is empty regardless of `self`.
	///
	/// # Panics
	/// Panics if `bbox` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_bbox(&self, bbox: &TileBBox) -> bool {
		assert_eq!(
			self.level, bbox.level,
			"Cannot compare TileBBox with level={} with TileBBox with level={}",
			self.level, bbox.level,
		);

		if bbox.is_empty() {
			return true; // empty set is a subset of any set
		}
		if self.is_empty() {
			return false;
		}

		// Safety: is_empty() checked above; getters are valid.
		self.x_min().unwrap() <= bbox.x_min().unwrap()
			&& self.x_max().unwrap() >= bbox.x_max().unwrap()
			&& self.y_min().unwrap() <= bbox.y_min().unwrap()
			&& self.y_max().unwrap() >= bbox.y_max().unwrap()
	}

	/// Returns `true` if every tile in `tree` is also in `self`.
	///
	/// Delegates to `includes_bbox` via `tree.to_bbox()`.
	#[must_use]
	pub fn includes_tree(&self, tree: &TileQuadtree) -> bool {
		self.includes_bbox(&tree.to_bbox())
	}

	/// Returns `true` if every tile in `cover` is also in `self`.
	///
	/// Delegates to `includes_bbox` via `cover.to_bbox()`.
	#[must_use]
	pub fn includes_cover(&self, cover: &TileCover) -> bool {
		self.includes_bbox(&cover.to_bbox())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;

	fn tc(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	#[test]
	fn test_contains() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;
		assert!(bbox.includes_coord(&tc(4, 6, 11)));
		assert!(!bbox.includes_coord(&tc(4, 4, 9)));
		Ok(())
	}

	#[test]
	fn should_determine_contains3_correctly() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;
		let valid_coord = tc(4, 6, 11);
		let invalid_coord_outside = tc(4, 4, 9);

		assert!(bbox.includes_coord(&valid_coord));
		assert!(!bbox.includes_coord(&invalid_coord_outside));

		Ok(())
	}

	#[test]
	#[should_panic(expected = "Cannot compare TileBBox with level=")]
	fn includes_coord_zoom_mismatch_panics() {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12).unwrap();
		let _ = bbox.includes_coord(&tc(5, 6, 11));
	}

	#[test]
	fn test_try_contains_bbox() -> Result<()> {
		let bbox_outer = TileBBox::from_min_and_max(5, 10, 10, 20, 20)?;
		let bbox_inner = TileBBox::from_min_and_max(5, 12, 12, 18, 18)?;
		let bbox_partial = TileBBox::from_min_and_max(5, 15, 15, 25, 25)?;
		let bbox_non_overlap = TileBBox::from_min_and_max(5, 21, 21, 22, 22)?;

		// Fully contained
		assert!(bbox_outer.includes_bbox(&bbox_inner));
		// Not fully contained (partial overlap)
		assert!(!bbox_outer.includes_bbox(&bbox_partial));
		// Not contained (no overlap)
		assert!(!bbox_outer.includes_bbox(&bbox_non_overlap));

		// Empty subset: any set includes the empty set; empty set includes nothing non-empty
		let empty_outer = TileBBox::new_empty(5)?;
		let empty_inner = TileBBox::new_empty(5)?;
		assert!(!empty_outer.includes_bbox(&bbox_inner));
		assert!(bbox_outer.includes_bbox(&empty_inner));
		assert!(empty_outer.includes_bbox(&empty_inner));

		Ok(())
	}

	#[test]
	#[should_panic(expected = "Cannot compare TileBBox with level=")]
	fn includes_bbox_zoom_mismatch_panics() {
		let bbox_outer = TileBBox::from_min_and_max(5, 10, 10, 20, 20).unwrap();
		let bbox_diff_level = TileBBox::from_min_and_max(6, 12, 12, 18, 18).unwrap();
		let _ = bbox_outer.includes_bbox(&bbox_diff_level);
	}
}
