//! Query methods for [`TileQuadtreePyramid`].

use super::TileQuadtreePyramid;
use crate::{GeoBBox, GeoCenter, TileBBox, TileCoord, TileQuadtree};
use anyhow::Result;
use versatiles_derive::context;

impl TileQuadtreePyramid {
	/// Returns a reference to the quadtree at the specified zoom level.
	///
	/// # Panics
	///
	/// Panics if `zoom` exceeds `MAX_ZOOM_LEVEL`.
	#[must_use]
	pub fn get_level(&self, zoom: u8) -> &TileQuadtree {
		&self.levels[zoom as usize]
	}

	/// Finds the minimum (lowest) non-empty zoom level.
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn get_level_min(&self) -> Option<u8> {
		self.levels.iter().find(|qt| !qt.is_empty()).map(TileQuadtree::zoom)
	}

	/// Finds the maximum (highest) non-empty zoom level.
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn get_level_max(&self) -> Option<u8> {
		self
			.levels
			.iter()
			.rev()
			.find(|qt| !qt.is_empty())
			.map(TileQuadtree::zoom)
	}

	/// Alias for [`get_level_min`](Self::get_level_min).
	#[must_use]
	pub fn get_zoom_min(&self) -> Option<u8> {
		self.get_level_min()
	}

	/// Alias for [`get_level_max`](Self::get_level_max).
	#[must_use]
	pub fn get_zoom_max(&self) -> Option<u8> {
		self.get_level_max()
	}

	/// Checks if the pyramid contains the given tile coordinate.
	///
	/// Returns `false` if the coordinate's zoom level has an empty quadtree,
	/// or if the tile is not in the quadtree.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		if let Some(qt) = self.levels.get(coord.level as usize) {
			qt.contains_tile(*coord).unwrap_or(false)
		} else {
			false
		}
	}

	/// Checks if the pyramid completely includes all tiles in the given bounding box.
	///
	/// # Errors
	///
	/// Returns an error if the bbox's zoom level exceeds `MAX_ZOOM_LEVEL`.
	pub fn includes_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		if let Some(qt) = self.levels.get(bbox.level as usize) {
			qt.contains_bbox(bbox)
		} else {
			Ok(false)
		}
	}

	/// Counts the total number of tiles across all zoom levels.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		self.levels.iter().map(TileQuadtree::tile_count).sum()
	}

	/// Returns `true` if all zoom levels are empty.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.levels.iter().all(TileQuadtree::is_empty)
	}

	/// Returns an iterator over the bounding boxes of all non-empty zoom levels.
	pub fn iter_levels(&self) -> impl Iterator<Item = TileBBox> + '_ {
		self
			.levels
			.iter()
			.filter(|qt| !qt.is_empty())
			.filter_map(TileQuadtree::bounds)
	}

	/// Returns a "good" zoom level heuristically — the highest level with more than 10 tiles.
	///
	/// Returns `None` if no level has more than 10 tiles.
	#[must_use]
	pub fn get_good_level(&self) -> Option<u8> {
		self
			.levels
			.iter()
			.rev()
			.find(|qt| qt.tile_count() > 10)
			.map(TileQuadtree::zoom)
	}

	/// Calculates a geographic center based on the bounding box at a middle zoom level.
	///
	/// Returns `None` if the pyramid is empty.
	#[must_use]
	pub fn get_geo_center(&self) -> Option<GeoCenter> {
		let bbox = self.get_geo_bbox()?;
		let zoom = (self.get_level_min()? + 2).min(self.get_level_max()?);
		let center_lon = f64::midpoint(bbox.x_min, bbox.x_max);
		let center_lat = f64::midpoint(bbox.y_min, bbox.y_max);
		Some(GeoCenter(center_lon, center_lat, zoom))
	}

	/// Returns a geographic bounding box covering the union of all non-empty levels.
	///
	/// Uses the highest non-empty zoom level's bounds for maximum precision, matching
	/// the approach of [`TileBBoxPyramid::get_geo_bbox`](crate::TileBBoxPyramid::get_geo_bbox).
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn get_geo_bbox(&self) -> Option<GeoBBox> {
		let max_zoom = self.get_level_max()?;
		self.levels[max_zoom as usize].to_geo_bbox()
	}

	/// Returns the intersection of `bbox` with the quadtree at its zoom level.
	///
	/// If the zoom level is out of range or the level is empty, returns an empty bbox.
	///
	/// # Errors
	///
	/// Returns an error if creating the intersected bbox fails.
	#[context("intersecting bbox {bbox:?} with pyramid")]
	pub fn intersected_bbox(&self, bbox: &TileBBox) -> Result<TileBBox> {
		if let Some(level_bounds) = self.levels.get(bbox.level as usize).and_then(TileQuadtree::bounds) {
			level_bounds.intersected_bbox(bbox)
		} else {
			TileBBox::new_empty(bbox.level)
		}
	}

	/// Checks whether the pyramid intersects (overlaps) the given bounding box.
	///
	/// Returns `false` if the bbox's zoom level is out of range.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		if let Some(qt) = self.levels.get(bbox.level as usize)
			&& let Some(level_bounds) = qt.bounds()
		{
			return level_bounds.intersects_bbox(bbox);
		}
		false
	}
}
