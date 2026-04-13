//! Query methods for [`TilePyramid`].

use super::TilePyramid;
use crate::{GeoBBox, GeoCenter, TileBBox, TileCoord, TileCover};
use anyhow::Result;

impl TilePyramid {
	/// Returns a reference to the [`TileCover`] at the given zoom level.
	#[must_use]
	pub fn get_level(&self, zoom: u8) -> &TileCover {
		&self.levels[zoom as usize]
	}

	/// Returns the bounding box of the given zoom level, or an empty bbox if
	/// the level is empty.
	#[must_use]
	pub fn get_level_bbox(&self, zoom: u8) -> TileBBox {
		self.levels[zoom as usize]
			.bounds()
			.unwrap_or_else(|| TileBBox::new_empty(zoom).expect("zoom must be ≤ MAX_ZOOM_LEVEL"))
	}

	/// Finds the minimum (lowest) non-empty zoom level.
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn get_level_min(&self) -> Option<u8> {
		self.levels.iter().find(|c| !c.is_empty()).map(TileCover::level)
	}

	/// Finds the maximum (highest) non-empty zoom level.
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn get_level_max(&self) -> Option<u8> {
		self.levels.iter().rev().find(|c| !c.is_empty()).map(TileCover::level)
	}

	/// Returns `true` if this pyramid contains the given tile coordinate.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		if let Some(cover) = self.levels.get(coord.level as usize) {
			cover.includes_coord(coord).unwrap()
		} else {
			false
		}
	}

	/// Returns `true` if all tiles in `bbox` are covered by this pyramid.
	///
	/// # Errors
	/// Returns an error if `bbox`'s level is out of range.
	pub fn includes_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		if let Some(cover) = self.levels.get(bbox.level as usize) {
			cover.includes_bbox(bbox)
		} else {
			Ok(false)
		}
	}

	/// Returns `true` if this pyramid completely includes every level of `other`.
	#[must_use]
	pub fn includes_pyramid(&self, other: &TilePyramid) -> bool {
		for cover_other in other.levels.iter().filter(|c| !c.is_empty()) {
			if cover_other
				.bounds()
				.is_some_and(|bounds| !self.includes_bbox(&bounds).unwrap())
			{
				return false;
			}
		}
		true
	}

	/// Returns `true` if the given `bbox` overlaps the coverage at its zoom level.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		if let Some(cover) = self.levels.get(bbox.level as usize) {
			cover.intersects_bbox(bbox)
		} else {
			false
		}
	}

	/// Returns `true` if any level of this pyramid overlaps the corresponding
	/// level of `other`.
	#[must_use]
	pub fn intersects_pyramid(&self, other: &TilePyramid) -> bool {
		self
			.levels
			.iter()
			.filter(|c| !c.is_empty())
			.any(|cover| cover.bounds().is_some_and(|bounds| other.intersects_bbox(&bounds)))
	}

	/// Returns the intersection of `bbox` with the coverage at `bbox`'s zoom
	/// level, or an empty bbox if the level is empty or out of range.
	///
	/// # Errors
	/// Returns an error if creating an empty bbox fails (should not happen for
	/// valid inputs).
	pub fn intersected_bbox(&self, bbox: &TileBBox) -> Result<TileBBox> {
		if let Some(level_bounds) = self.levels.get(bbox.level as usize).and_then(TileCover::bounds) {
			return level_bounds.intersected_bbox(bbox);
		}
		TileBBox::new_empty(bbox.level)
	}

	/// Counts the total number of tiles across all zoom levels.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		self.levels.iter().map(TileCover::count_tiles).sum()
	}

	/// Counts the total number of internal quadtree nodes across all `Tree`
	/// levels.
	#[must_use]
	pub fn count_nodes(&self) -> u64 {
		self
			.levels
			.iter()
			.filter_map(TileCover::as_tree)
			.map(crate::TileQuadtree::count_nodes)
			.sum()
	}

	/// Returns `true` if all levels are empty.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.levels.iter().all(TileCover::is_empty)
	}

	/// Returns an iterator over the bounding boxes of all non-empty zoom levels.
	pub fn iter_levels(&self) -> impl Iterator<Item = TileBBox> + '_ {
		self
			.levels
			.iter()
			.filter(|c| !c.is_empty())
			.filter_map(TileCover::bounds)
	}

	/// Returns an iterator over the bounding box for **every** zoom level
	/// (0–MAX_ZOOM_LEVEL), including empty ones.
	///
	/// Empty levels yield an empty [`TileBBox`] at that level. Used by the
	/// tile-container traversal logic.
	pub fn iter_all_level_bboxes(&self) -> impl Iterator<Item = TileBBox> + '_ {
		self.levels.iter().map(|c| {
			c.bounds()
				.unwrap_or_else(|| TileBBox::new_empty(c.level()).expect("level must be ≤ MAX_ZOOM_LEVEL"))
		})
	}

	/// Returns a geographic bounding box covering the union of all non-empty
	/// levels (using the highest non-empty level for maximum precision).
	///
	/// Returns `None` if all levels are empty.
	#[must_use]
	pub fn get_geo_bbox(&self) -> Option<GeoBBox> {
		let max_zoom = self.get_level_max()?;
		self.levels[max_zoom as usize].to_geo_bbox()
	}

	/// Calculates a geographic center based on the bounding box at a middle
	/// zoom level.
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

	/// Returns a tile-count-weighted geographic bounding box.
	///
	/// # Errors
	/// Returns an error if the pyramid is empty.
	pub fn weighted_bbox(&self) -> Result<GeoBBox> {
		use anyhow::ensure;
		let mut x_min_sum = 0.0_f64;
		let mut y_min_sum = 0.0_f64;
		let mut x_max_sum = 0.0_f64;
		let mut y_max_sum = 0.0_f64;
		let mut weight_sum = 0.0_f64;
		for cover in &self.levels {
			if let Some(geo) = cover.to_geo_bbox() {
				let weight = cover.count_tiles() as f64;
				x_min_sum += geo.x_min * weight;
				y_min_sum += geo.y_min * weight;
				x_max_sum += geo.x_max * weight;
				y_max_sum += geo.y_max * weight;
				weight_sum += weight;
			}
		}
		ensure!(weight_sum > 0.0, "Cannot compute weighted bbox for an empty pyramid");
		GeoBBox::new(
			x_min_sum / weight_sum,
			y_min_sum / weight_sum,
			x_max_sum / weight_sum,
			y_max_sum / weight_sum,
		)
	}
}
