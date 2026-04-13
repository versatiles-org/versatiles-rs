//! Query methods for [`TileQuadtree`].

use super::constructors::{check_bbox_zoom, check_coord_zoom};
use super::{BBox, TileQuadtree};
use crate::{GeoBBox, TileBBox, TileCoord};
use anyhow::Result;

impl TileQuadtree {
	/// Return true if the quadtree contains no tiles.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.root.is_empty()
	}

	/// Return true if the quadtree contains all tiles at its zoom level.
	#[must_use]
	pub fn is_full(&self) -> bool {
		self.root.is_full()
	}

	/// Count the total number of tiles in the quadtree.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		self.root.count_tiles(self.level)
	}

	/// Count the number of internal (Partial) nodes in the quadtree.
	#[must_use]
	pub fn count_nodes(&self) -> u64 {
		self.root.count_nodes()
	}

	/// Return the tightest axis-aligned [`TileBBox`] containing all tiles,
	/// or `None` if the quadtree is empty.
	#[must_use]
	pub fn bounds(&self) -> Option<TileBBox> {
		let size = 1u64 << self.level;
		self.root.bounds((0, 0), size).map(|(x0, y0, x1, y1)| {
			TileBBox::from_min_and_max(
				self.level,
				u32::try_from(x0).unwrap(),
				u32::try_from(y0).unwrap(),
				u32::try_from(x1 - 1).unwrap(),
				u32::try_from(y1 - 1).unwrap(),
			)
			.unwrap()
		})
	}

	/// Convert the covered area to a geographic [`GeoBBox`], or `None` if empty.
	#[must_use]
	pub fn to_geo_bbox(&self) -> Option<GeoBBox> {
		self.bounds().map(|bb| bb.to_geo_bbox().unwrap())
	}

	/// Check whether a specific tile coordinate is in this quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's level doesn't match this quadtree's zoom.
	pub fn includes_coord(&self, coord: &TileCoord) -> Result<bool> {
		check_coord_zoom(coord, self.level)?;
		let size = 1u64 << self.level;
		Ok(self
			.root
			.includes_coord((0, 0), size, (u64::from(coord.x), u64::from(coord.y))))
	}

	/// Check whether all tiles in `bbox` are in this quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's level doesn't match this quadtree's zoom.
	pub fn includes_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		check_bbox_zoom(bbox, self.level)?;
		let size = 1u64 << self.level;
		let bbox = if let Some(bbox) = BBox::new(bbox) {
			bbox
		} else {
			return Ok(true);
		};
		Ok(self.root.includes_bbox(0, 0, size, bbox))
	}

	/// Check whether this quadtree has any tiles in common with `other`.
	///
	/// # Errors
	/// Returns an error if the zoom levels don't match.
	pub fn intersects_tree(&self, other: &TileQuadtree) -> Result<bool> {
		anyhow::ensure!(
			self.level == other.level,
			"Cannot intersect quadtrees with different zoom levels: {} vs {}",
			self.level,
			other.level
		);
		Ok(self.root.intersects_tree(&other.root))
	}
}
