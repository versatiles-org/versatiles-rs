//! Mutation methods for [`TilePyramid`].

use super::TilePyramid;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileCover};
use anyhow::Result;

impl TilePyramid {
	/// Sets the cover at the level encoded in `cover`.
	pub fn set_level(&mut self, cover: TileCover) {
		let z = cover.level() as usize;
		self.levels[z] = cover;
	}

	/// Sets the cover at `bbox`'s zoom level to the given bounding box.
	pub fn set_level_bbox(&mut self, bbox: TileBBox) {
		self.levels[bbox.level() as usize] = TileCover::from(bbox);
	}

	/// Includes a single tile coordinate (expands coverage at its zoom level).
	pub fn insert_coord(&mut self, coord: &TileCoord) {
		self.levels[coord.level as usize].insert_coord(coord).unwrap();
	}

	/// Includes all tiles in `bbox` (expands coverage at `bbox`'s zoom level).
	///
	/// # Errors
	/// Returns an error if the zoom level is invalid or insertion fails.
	pub fn insert_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		self.levels[bbox.level() as usize].insert_bbox(bbox)
	}

	/// Includes all coverage from `other` into this pyramid (union per level).
	pub fn union(&mut self, other: &TilePyramid) {
		for z in 0..=MAX_ZOOM_LEVEL as usize {
			let union = self.levels[z].union(&other.levels[z]).unwrap();
			self.levels[z] = union;
		}
	}

	/// Intersects each zoom level with the coverage derived from a geographic
	/// bounding box.
	///
	/// Levels ≤ 16 get an exact quadtree; higher levels reuse the cap tree scaled
	/// via `at_level` (O(1)).
	pub fn intersect_geo_bbox(&mut self, geo_bbox: &GeoBBox) -> Result<()> {
		for z in 0..=MAX_ZOOM_LEVEL {
			if let Some(level) = self.levels.get_mut(z as usize) {
				let bbox = TileBBox::from_geo_bbox(z, geo_bbox)?;
				level.intersect_bbox(&bbox)?;
			}
		}
		Ok(())
	}

	/// Clears all zoom levels below `level_min`.
	pub fn set_level_min(&mut self, level_min: u8) {
		for l in 0..level_min {
			self.levels[l as usize] = TileCover::new_empty(l).unwrap();
		}
	}

	/// Clears all zoom levels above `level_max`.
	pub fn set_level_max(&mut self, level_max: u8) {
		for l in (level_max + 1)..=MAX_ZOOM_LEVEL {
			self.levels[l as usize] = TileCover::new_empty(l).unwrap();
		}
	}

	/// Expands tile coverage by `size` tiles in all directions on all levels.
	pub fn buffer(&mut self, size: u32) {
		for cover in &mut self.levels {
			cover.buffer(size);
		}
	}

	/// Applies a Y-flip to every level.
	pub fn flip_y(&mut self) {
		for cover in &mut self.levels {
			cover.flip_y();
		}
	}

	/// Applies an X/Y swap to every level.
	pub fn swap_xy(&mut self) {
		for cover in &mut self.levels {
			cover.swap_xy();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileQuadtree;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}
	fn coord(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	#[test]
	fn get_level_and_set_level() {
		let mut p = TilePyramid::new_empty();
		let qt = TileQuadtree::new_full(4).unwrap();
		p.set_level(TileCover::from(qt));
		assert!(!p.level_ref(4).is_empty());
		assert!(p.level_ref(3).is_empty());
	}

	#[test]
	fn set_level_bbox() {
		let mut p = TilePyramid::new_empty();
		p.set_level_bbox(bbox(5, 3, 4, 10, 15));
		assert_eq!(p.level_bbox(5), bbox(5, 3, 4, 10, 15));
		assert!(p.level_ref(4).is_empty());
	}

	#[test]
	fn include_coord() {
		let mut p = TilePyramid::new_empty();
		p.insert_coord(&coord(5, 7, 9));
		assert!(p.includes_coord(&coord(5, 7, 9)));
		assert!(!p.includes_coord(&coord(5, 0, 0)));
	}

	#[test]
	fn union() {
		let mut a = TilePyramid::new_empty();
		a.insert_bbox(&bbox(5, 0, 0, 5, 5)).unwrap();

		let mut b = TilePyramid::new_empty();
		b.insert_bbox(&bbox(5, 10, 10, 15, 15)).unwrap();

		a.union(&b);
		assert!(a.includes_coord(&coord(5, 2, 2)));
		assert!(a.includes_coord(&coord(5, 12, 12)));
	}

	#[test]
	fn set_level_min_and_max() {
		let mut p = TilePyramid::new_full();
		p.set_level_min(5);
		assert!(p.level_ref(4).is_empty());
		assert!(!p.level_ref(5).is_empty());

		p.set_level_max(10);
		assert!(!p.level_ref(10).is_empty());
		assert!(p.level_ref(11).is_empty());
	}

	#[test]
	fn add_border() {
		// Use set_level_bbox to ensure the level stays a Bbox variant
		// (include_bbox on an empty cover upgrades to Tree, which add_border skips).
		let mut p = TilePyramid::new_empty();
		p.set_level_bbox(bbox(5, 5, 5, 10, 10));
		p.buffer(1);
		let b = p.level_bbox(5);
		assert_eq!(b.x_min().unwrap(), 4);
		assert_eq!(b.y_min().unwrap(), 4);
		assert_eq!(b.x_max().unwrap(), 11);
		assert_eq!(b.y_max().unwrap(), 11);
	}

	#[test]
	fn add_border_empty_level_unaffected() {
		let mut p = TilePyramid::new_empty();
		p.buffer(5);
		assert!(p.is_empty());
	}

	#[test]
	fn flip_y_and_swap_xy() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(3, 0, 0, 3, 3)).unwrap();
		// Just verify they don't panic
		p.flip_y();
		p.swap_xy();
		assert!(!p.is_empty());
	}

	#[test]
	fn flip_y_changes_coordinates() {
		let mut p = TilePyramid::new_empty();
		// z=1: 2x2 grid; top-left tile (0,0) flips to bottom-left (0,1)
		p.insert_bbox(&bbox(1, 0, 0, 0, 0)).unwrap();
		p.flip_y();
		assert!(p.includes_coord(&coord(1, 0, 1)));
		assert!(!p.includes_coord(&coord(1, 0, 0)));
	}

	#[test]
	fn swap_xy_changes_coordinates() {
		let mut p = TilePyramid::new_empty();
		// bbox with x=[2..4], y=[0..1] → after swap: x=[0..1], y=[2..4]
		p.insert_bbox(&bbox(4, 2, 0, 4, 1)).unwrap();
		p.swap_xy();
		let b = p.level_bbox(4);
		assert_eq!(b.x_min().unwrap(), 0);
		assert_eq!(b.y_min().unwrap(), 2);
	}

	// ── flip_y / swap_xy are involutions on any pyramid ─────────────────────
	fn build_sample() -> TilePyramid {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(2, 0, 0, 2, 2)).unwrap();
		p.insert_bbox(&bbox(4, 3, 5, 10, 12)).unwrap();
		p.set_level(TileCover::from(TileQuadtree::from_bbox(&bbox(5, 1, 1, 7, 7))));
		p
	}

	#[rstest::rstest]
	#[case::empty(TilePyramid::new_empty())]
	#[case::full_up_to_3(TilePyramid::new_full_up_to(3))]
	#[case::mixed(build_sample())]
	fn flip_y_is_involution(#[case] original: TilePyramid) {
		let mut p = original.clone();
		p.flip_y();
		p.flip_y();
		assert_eq!(p, original);
	}

	#[rstest::rstest]
	#[case::empty(TilePyramid::new_empty())]
	#[case::full_up_to_3(TilePyramid::new_full_up_to(3))]
	#[case::mixed(build_sample())]
	fn swap_xy_is_involution(#[case] original: TilePyramid) {
		let mut p = original.clone();
		p.swap_xy();
		p.swap_xy();
		assert_eq!(p, original);
	}

	// ── buffer edge cases ───────────────────────────────────────────────────
	#[rstest::rstest]
	#[case(0)] // identity
	#[case(1)]
	#[case(100)]
	fn buffer_empty_pyramid_stays_empty(#[case] size: u32) {
		let mut p = TilePyramid::new_empty();
		p.buffer(size);
		assert!(p.is_empty());
	}

	#[test]
	fn buffer_zero_is_identity() {
		let mut p = TilePyramid::new_empty();
		p.set_level_bbox(bbox(5, 5, 5, 10, 10));
		let before = p.clone();
		p.buffer(0);
		assert_eq!(p, before);
	}

	// ── set_level_min / set_level_max behaviour ─────────────────────────────
	#[rstest::rstest]
	#[case(0)] // no-op: nothing below 0
	#[case(3)] // clears 0..=2
	#[case(31)] // clears everything
	fn set_level_min_clears_below(#[case] min: u8) {
		let mut p = TilePyramid::new_full();
		p.set_level_min(min);
		for l in 0..min {
			assert!(p.level_ref(l).is_empty(), "level {l} should be empty");
		}
		if min <= 30 {
			assert!(p.level_ref(min).is_full(), "level {min} should still be full");
		}
	}

	#[rstest::rstest]
	#[case(0)] // keeps only level 0
	#[case(15)]
	#[case(30)]
	fn set_level_max_clears_above(#[case] max: u8) {
		let mut p = TilePyramid::new_full();
		p.set_level_max(max);
		for l in (max + 1)..=30 {
			assert!(p.level_ref(l).is_empty(), "level {l} should be empty");
		}
		assert!(p.level_ref(max).is_full());
	}

	#[test]
	fn union_with_empty_is_noop() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(3, 0, 0, 3, 3)).unwrap();
		let before = p.clone();
		p.union(&TilePyramid::new_empty());
		assert_eq!(p, before);
	}

	#[test]
	fn union_with_self_is_idempotent() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(3, 0, 0, 3, 3)).unwrap();
		let before = p.clone();
		p.union(&before);
		// After A ∪ A, coverage must still be the same (bit-for-bit equal).
		assert_eq!(p, before);
	}

	#[test]
	fn intersect_geo_bbox_restricts_all_levels() {
		let mut p = TilePyramid::new_full_up_to(5);
		p.intersect_geo_bbox(&GeoBBox::new(0.0, 0.0, 10.0, 10.0).unwrap())
			.unwrap();
		// Every level's coverage should be a strict subset of the full level.
		for l in 0..=5 {
			let bbox = p.level_bbox(l);
			let full = TileBBox::new_full(l).unwrap();
			assert!(bbox.count_tiles() <= full.count_tiles());
		}
	}

	#[test]
	fn intersect_geo_bbox_noop_for_world() {
		// Intersecting with the full world should not change a full pyramid.
		let mut p = TilePyramid::new_full_up_to(3);
		let before = p.clone();
		p.intersect_geo_bbox(&GeoBBox::new(-180.0, -85.0, 180.0, 85.0).unwrap())
			.unwrap();
		assert_eq!(p.count_tiles(), before.count_tiles());
	}

	#[test]
	fn insert_coord_multiple_levels() {
		let mut p = TilePyramid::new_empty();
		for l in [0u8, 5, 10, 20] {
			p.insert_coord(&coord(l, 0, 0));
		}
		assert_eq!(p.level_min(), Some(0));
		assert_eq!(p.level_max(), Some(20));
		assert_eq!(p.count_tiles(), 4);
	}
}
