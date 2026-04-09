//! Mutation methods for [`TileQuadtreePyramid`].

use super::TileQuadtreePyramid;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileQuadtree};
use anyhow::Result;
use versatiles_derive::context;

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
	/// # Errors
	///
	/// Returns an error if the geographic coordinates are invalid.
	#[context("Failed to intersect {self} with {geo_bbox:?}")]
	pub fn intersect_geo_bbox(&mut self, geo_bbox: &GeoBBox) -> Result<()> {
		for z in 0..=MAX_ZOOM_LEVEL {
			let geo_qt = TileQuadtree::from_geo(z, geo_bbox)?;
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
	pub fn set_zoom_min(&mut self, zoom_min: u8) {
		for z in 0..zoom_min as usize {
			self.levels[z] = TileQuadtree::new_empty(u8::try_from(z).expect("zoom level index exceeds u8::MAX"));
		}
	}

	/// Clears (sets to empty) all zoom levels above `zoom_max`.
	pub fn set_zoom_max(&mut self, zoom_max: u8) {
		for z in (zoom_max as usize + 1)..=MAX_ZOOM_LEVEL as usize {
			self.levels[z] = TileQuadtree::new_empty(u8::try_from(z).expect("zoom level index exceeds u8::MAX"));
		}
	}
}
