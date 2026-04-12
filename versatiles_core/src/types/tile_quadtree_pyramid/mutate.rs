//! Mutation methods for [`TileQuadtreePyramid`].

use super::TileQuadtreePyramid;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileQuadtree};
use anyhow::Result;
use versatiles_derive::context;

/// Zoom level at which the geo-quadtree is materialised exactly.
/// For higher zoom levels, the tree built at this cap is scaled up via `level_down`
/// (O(1) per step) rather than building a new O(perimeter_tiles) tree at each level.
const MAX_QUADTREE_INTERSECT_ZOOM: u8 = 16;

impl TileQuadtreePyramid {
	/// Sets the quadtree at the zoom level matching `qt.zoom()`.
	///
	/// # Panics
	///
	/// Panics if `qt.zoom()` exceeds `MAX_ZOOM_LEVEL`.
	pub fn set_level(&mut self, qt: TileQuadtree) {
		let zoom = qt.zoom();
		self.levels[zoom as usize] = qt;
	}

	/// Includes a single tile coordinate in the pyramid by inserting it into the
	/// quadtree at the coordinate's zoom level.
	pub fn include_coord(&mut self, coord: &TileCoord) {
		self.levels[coord.level as usize].insert_tile(*coord).unwrap();
	}

	/// Includes all tiles in the given bounding box in the pyramid.
	///
	/// # Errors
	///
	/// Returns an error if the bbox's zoom level is invalid or doesn't match.
	pub fn include_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		self.levels[bbox.level as usize].insert_bbox(bbox)
	}

	/// Includes all coverage from another `TileQuadtreePyramid` into this one.
	///
	/// For each zoom level, computes the union of the two quadtrees.
	pub fn include_pyramid(&mut self, other: &TileQuadtreePyramid) {
		for z in 0..=MAX_ZOOM_LEVEL as usize {
			let union = self.levels[z].union(&other.levels[z]).unwrap();
			self.levels[z] = union;
		}
	}

	/// Intersects each zoom level with the coverage derived from the given geographic
	/// bounding box.
	///
	/// For zoom levels up to `MAX_QUADTREE_INTERSECT_ZOOM` (16), a full quadtree is built
	/// at that zoom and intersected precisely. For higher zoom levels the cap-zoom tree is
	/// scaled up via `level_down` — which is O(1) per step because it only increments the
	/// zoom counter without touching the node structure. This avoids materialising an
	/// O(perimeter_tiles) tree at each high zoom level (which was the old "fast-path" bug:
	/// at zoom 28 a 60° bbox has ~44 M boundary tiles, producing ~hundreds of MB of nodes).
	///
	/// The scaled tree is a slight over-approximation at the finest levels (each leaf of
	/// the cap-zoom tree represents 2^(z − cap) actual tiles), but this is intentional and
	/// acceptable — exact tile-level precision is only maintained up to zoom 16.
	///
	/// # Errors
	///
	/// Returns an error if the geographic coordinates are invalid.
	#[context("Failed to intersect {self} with {geo_bbox:?}")]
	pub fn intersect_geo_bbox(&mut self, geo_bbox: &GeoBBox) -> Result<()> {
		// Build the geo-quadtree once at the cap zoom; reuse for all higher levels.
		let cap_geo_qt = TileQuadtree::from_geo(MAX_QUADTREE_INTERSECT_ZOOM, geo_bbox)?;

		for z in 0..=MAX_ZOOM_LEVEL {
			if self.levels[z as usize].is_empty() {
				continue;
			}
			let geo_qt = if z <= MAX_QUADTREE_INTERSECT_ZOOM {
				TileQuadtree::from_geo(z, geo_bbox)?
			} else {
				// Scale the cap tree up to zoom z.  level_down is O(1) (increments zoom
				// counter only), so this is O((z − cap) + clone_of_cap_tree).
				cap_geo_qt.at_level(z)
			};
			let intersected = self.levels[z as usize].intersection(&geo_qt)?;
			self.levels[z as usize] = intersected;
		}
		Ok(())
	}

	/// Intersects (in-place) this pyramid with another `TileQuadtreePyramid`.
	///
	/// For each zoom level, computes the intersection of the two quadtrees.
	///
	/// # Errors
	///
	/// Returns an error if any level intersection fails.
	pub fn intersect(&mut self, other: &TileQuadtreePyramid) -> Result<()> {
		for z in 0..=MAX_ZOOM_LEVEL as usize {
			let intersected = self.levels[z].intersection(&other.levels[z])?;
			self.levels[z] = intersected;
		}
		Ok(())
	}

	/// Clears (sets to empty) all zoom levels below `zoom_min`.
	#[allow(clippy::cast_possible_truncation)] // z ≤ MAX_ZOOM_LEVEL (30) < u8::MAX
	pub fn set_zoom_min(&mut self, zoom_min: u8) {
		for z in 0..zoom_min as usize {
			self.levels[z] = TileQuadtree::new_empty(z as u8);
		}
	}

	/// Clears (sets to empty) all zoom levels above `zoom_max`.
	#[allow(clippy::cast_possible_truncation)] // z ≤ MAX_ZOOM_LEVEL (30) < u8::MAX
	pub fn set_zoom_max(&mut self, zoom_max: u8) {
		for z in (zoom_max as usize + 1)..=MAX_ZOOM_LEVEL as usize {
			self.levels[z] = TileQuadtree::new_empty(z as u8);
		}
	}

	/// Applies a Y-flip to all levels' bounding boxes, then rebuilds the quadtrees from
	/// those flipped bboxes (lossy: non-rectangular coverage becomes rectangular).
	///
	/// # Errors
	///
	/// Returns an error if rebuilding the quadtree pyramid from the flipped bboxes fails.
	pub fn flip_y(&mut self) -> Result<()> {
		let mut bbox_pyramid = self.to_bbox_pyramid();
		bbox_pyramid.flip_y();
		*self = Self::from_bbox_pyramid(&bbox_pyramid)?;
		Ok(())
	}

	/// Applies an X/Y swap to all levels' bounding boxes, then rebuilds the quadtrees from
	/// those swapped bboxes (lossy: non-rectangular coverage becomes rectangular).
	///
	/// # Errors
	///
	/// Returns an error if rebuilding the quadtree pyramid from the swapped bboxes fails.
	pub fn swap_xy(&mut self) -> Result<()> {
		let mut bbox_pyramid = self.to_bbox_pyramid();
		bbox_pyramid.swap_xy();
		*self = Self::from_bbox_pyramid(&bbox_pyramid)?;
		Ok(())
	}
}
