//! Mutation methods for [`TilePyramid`].

use super::TilePyramid;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileCover, TileQuadtree};
use anyhow::Result;
use versatiles_derive::context;

/// Zoom level at which the geo-quadtree is materialised exactly.
/// Above this level the cap tree is scaled via `at_level` (O(1)).
const MAX_QUADTREE_INTERSECT_ZOOM: u8 = 16;

impl TilePyramid {
	/// Sets the cover at the level encoded in `cover`.
	pub fn set_level(&mut self, cover: TileCover) {
		let z = cover.level() as usize;
		self.levels[z] = cover;
	}

	/// Sets the cover at `bbox`'s zoom level to the given bounding box.
	pub fn set_level_bbox(&mut self, bbox: TileBBox) {
		self.levels[bbox.level as usize] = TileCover::from(bbox);
	}

	/// Includes a single tile coordinate (expands coverage at its zoom level).
	pub fn include_coord(&mut self, coord: &TileCoord) {
		self.levels[coord.level as usize].include_coord(coord).unwrap();
	}

	/// Includes all tiles in `bbox` (expands coverage at `bbox`'s zoom level).
	///
	/// # Errors
	/// Returns an error if the zoom level is invalid or insertion fails.
	pub fn include_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		self.levels[bbox.level as usize].include_bbox(bbox)
	}

	/// Includes all coverage from `other` into this pyramid (union per level).
	pub fn include_pyramid(&mut self, other: &TilePyramid) {
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
	///
	/// # Errors
	/// Returns an error if the geographic coordinates are invalid.
	#[context("Failed to intersect {self} with {geo_bbox:?}")]
	pub fn intersect_geo_bbox(&mut self, geo_bbox: &GeoBBox) -> Result<()> {
		let cap_geo_qt = TileQuadtree::from_geo(MAX_QUADTREE_INTERSECT_ZOOM, geo_bbox)?;

		for z in 0..=MAX_ZOOM_LEVEL {
			if self.levels[z as usize].is_empty() {
				continue;
			}
			let geo_qt = if z <= MAX_QUADTREE_INTERSECT_ZOOM {
				TileQuadtree::from_geo(z, geo_bbox)?
			} else {
				cap_geo_qt.at_level(z)
			};
			let geo_cover = TileCover::from(geo_qt);
			let intersected = self.levels[z as usize].intersection(&geo_cover)?;
			self.levels[z as usize] = intersected;
		}
		Ok(())
	}

	/// Intersects (in place) this pyramid with `other` (intersection per level).
	///
	/// # Errors
	/// Returns an error if any level intersection fails.
	pub fn intersect(&mut self, other: &TilePyramid) -> Result<()> {
		for z in 0..=MAX_ZOOM_LEVEL as usize {
			let intersected = self.levels[z].intersection(&other.levels[z])?;
			self.levels[z] = intersected;
		}
		Ok(())
	}

	/// Clears all zoom levels below `zoom_min`.
	#[allow(clippy::cast_possible_truncation)]
	pub fn set_level_min(&mut self, zoom_min: u8) {
		for z in 0..zoom_min as usize {
			self.levels[z] = TileCover::new_empty(z as u8).unwrap();
		}
	}

	/// Clears all zoom levels above `zoom_max`.
	#[allow(clippy::cast_possible_truncation)]
	pub fn set_level_max(&mut self, zoom_max: u8) {
		for z in (zoom_max as usize + 1)..=MAX_ZOOM_LEVEL as usize {
			self.levels[z] = TileCover::new_empty(z as u8).unwrap();
		}
	}

	/// Expands `Bbox` levels by `(x_min, y_min, x_max, y_max)` tiles.
	///
	/// `Tree` levels are unaffected (exact subtraction would require converting
	/// back to bbox, which is lossy).
	pub fn add_border(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) {
		for cover in &mut self.levels {
			if let TileCover::Bbox(b) = cover {
				b.expand_by(x_min, y_min, x_max, y_max);
			}
		}
	}

	/// Applies a Y-flip to every level.
	///
	/// For `Bbox` levels this is exact. For `Tree` levels it is lossy (rounds
	/// through the bounding rectangle).
	///
	/// # Errors
	/// Returns an error if rebuilding any flipped level fails.
	pub fn flip_y(&mut self) -> Result<()> {
		for cover in &mut self.levels {
			match cover {
				TileCover::Bbox(b) => b.flip_y(),
				TileCover::Tree(t) => {
					// Lossy: round-trip through bounding box.
					if let Some(mut bbox) = t.bounds() {
						bbox.flip_y();
						*cover = TileCover::from(bbox);
					}
				}
			}
		}
		Ok(())
	}

	/// Applies an X/Y swap to every level.
	///
	/// For `Bbox` levels this is exact. For `Tree` levels it is lossy (rounds
	/// through the bounding rectangle).
	///
	/// # Errors
	/// Returns an error if rebuilding any swapped level fails.
	pub fn swap_xy(&mut self) -> Result<()> {
		for cover in &mut self.levels {
			match cover {
				TileCover::Bbox(b) => b.swap_xy(),
				TileCover::Tree(t) => {
					// Lossy: round-trip through bounding box.
					if let Some(mut bbox) = t.bounds() {
						bbox.swap_xy();
						*cover = TileCover::from(bbox);
					}
				}
			}
		}
		Ok(())
	}
}
