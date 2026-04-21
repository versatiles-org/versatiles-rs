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
}
