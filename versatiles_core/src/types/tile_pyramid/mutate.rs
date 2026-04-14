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

	/// Intersects (in place) this pyramid with `other` (intersection per level).
	///
	/// # Errors
	/// Returns an error if any level intersection fails.
	pub fn intersect(&mut self, other: &TilePyramid) -> Result<()> {
		for l in 0..=MAX_ZOOM_LEVEL as usize {
			let intersected = self.levels[l].intersection(&other.levels[l])?;
			self.levels[l] = intersected;
		}
		Ok(())
	}

	/// Clears all zoom levels below `level_min`.
	#[allow(clippy::cast_possible_truncation)]
	pub fn set_level_min(&mut self, level_min: u8) {
		for l in 0..level_min as usize {
			self.levels[l] = TileCover::new_empty(l as u8).unwrap();
		}
	}

	/// Clears all zoom levels above `level_max`.
	#[allow(clippy::cast_possible_truncation)]
	pub fn set_level_max(&mut self, level_max: u8) {
		for l in (level_max as usize + 1)..=MAX_ZOOM_LEVEL as usize {
			self.levels[l] = TileCover::new_empty(l as u8).unwrap();
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
