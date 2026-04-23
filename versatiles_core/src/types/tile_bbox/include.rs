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
		coord.x >= self.x_min().expect("bbox is non-empty")
			&& coord.x <= self.x_max().expect("bbox is non-empty")
			&& coord.y >= self.y_min().expect("bbox is non-empty")
			&& coord.y <= self.y_max().expect("bbox is non-empty")
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
		self.x_min().expect("bbox is non-empty") <= bbox.x_min().expect("bbox is non-empty")
			&& self.x_max().expect("bbox is non-empty") >= bbox.x_max().expect("bbox is non-empty")
			&& self.y_min().expect("bbox is non-empty") <= bbox.y_min().expect("bbox is non-empty")
			&& self.y_max().expect("bbox is non-empty") >= bbox.y_max().expect("bbox is non-empty")
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
	use rstest::rstest;

	fn tc(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	fn bb(z: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(z, x0, y0, x1, y1).unwrap()
	}

	/// `includes_coord` against bbox(4, 5,10,7,12).
	#[rstest]
	#[case::inside(tc(4, 6, 11), true)]
	#[case::outside_upper_left(tc(4, 4, 9), false)]
	#[case::outside_lower_right(tc(4, 8, 13), false)]
	#[case::on_min_corner(tc(4, 5, 10), true)]
	#[case::on_max_corner(tc(4, 7, 12), true)]
	fn includes_coord_cases(#[case] c: TileCoord, #[case] expected: bool) {
		assert_eq!(bb(4, 5, 10, 7, 12).includes_coord(&c), expected);
	}

	#[test]
	#[should_panic(expected = "Cannot compare TileBBox with level=")]
	fn includes_coord_zoom_mismatch_panics() {
		let _ = bb(4, 5, 10, 7, 12).includes_coord(&tc(5, 6, 11));
	}

	/// `outer.includes_bbox(inner)` with outer = bbox(5, 10,10,20,20) unless the
	/// case says otherwise.
	#[rstest]
	#[case::fully_contained(bb(5, 10, 10, 20, 20), bb(5, 12, 12, 18, 18), true)]
	#[case::partial_overlap(bb(5, 10, 10, 20, 20), bb(5, 15, 15, 25, 25), false)]
	#[case::disjoint(bb(5, 10, 10, 20, 20), bb(5, 21, 21, 22, 22), false)]
	#[case::empty_inner_is_subset(bb(5, 10, 10, 20, 20), TileBBox::new_empty(5).unwrap(), true)]
	#[case::empty_outer_excludes_nonempty(TileBBox::new_empty(5).unwrap(), bb(5, 12, 12, 18, 18), false)]
	#[case::empty_outer_includes_empty(TileBBox::new_empty(5).unwrap(), TileBBox::new_empty(5).unwrap(), true)]
	fn includes_bbox_cases(#[case] outer: TileBBox, #[case] inner: TileBBox, #[case] expected: bool) {
		assert_eq!(outer.includes_bbox(&inner), expected);
	}

	#[test]
	#[should_panic(expected = "Cannot compare TileBBox with level=")]
	fn includes_bbox_zoom_mismatch_panics() {
		let _ = bb(5, 10, 10, 20, 20).includes_bbox(&bb(6, 12, 12, 18, 18));
	}
}
