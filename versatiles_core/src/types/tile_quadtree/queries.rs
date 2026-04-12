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
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileQuadtree;
	/// assert_eq!(TileQuadtree::new_full(2).tile_count(), 16);
	/// assert_eq!(TileQuadtree::new_empty(2).tile_count(), 0);
	/// ```
	#[must_use]
	pub fn tile_count(&self) -> u64 {
		self.root.count(self.zoom)
	}

	/// Count the number of internal (Partial) nodes in the quadtree.
	///
	/// `Full` and `Empty` nodes are terminal and not counted; only `Partial`
	/// nodes that subdivide their region into four children are counted.
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileQuadtree;
	/// assert_eq!(TileQuadtree::new_full(5).node_count(), 0);
	/// assert_eq!(TileQuadtree::new_empty(5).node_count(), 0);
	/// ```
	#[must_use]
	pub fn node_count(&self) -> u64 {
		self.root.count_partial()
	}

	/// Return the tightest axis-aligned [`TileBBox`] containing all tiles,
	/// or `None` if the quadtree is empty.
	#[must_use]
	pub fn bounds(&self) -> Option<TileBBox> {
		let size = 1u64 << self.zoom;
		self.root.bounds(0, 0, size).map(|(x0, y0, x1, y1)| {
			TileBBox::from_min_and_max(
				self.zoom,
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
	pub fn contains_tile(&self, coord: TileCoord) -> Result<bool> {
		check_coord_zoom(coord, self.zoom)?;
		let size = 1u64 << self.zoom;
		Ok(self
			.root
			.contains_tile(0, 0, size, u64::from(coord.x), u64::from(coord.y)))
	}

	/// Check whether all tiles in `bbox` are in this quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's level doesn't match this quadtree's zoom.
	pub fn contains_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		check_bbox_zoom(bbox, self.zoom)?;
		if bbox.is_empty() {
			return Ok(true);
		}
		let size = 1u64 << self.zoom;
		let bx_min = u64::from(bbox.x_min()?);
		let by_min = u64::from(bbox.y_min()?);
		let bx_max = u64::from(bbox.x_max()?) + 1;
		let by_max = u64::from(bbox.y_max()?) + 1;
		Ok(self.root.contains_bbox(
			0,
			0,
			size,
			BBox {
				x_min: bx_min,
				y_min: by_min,
				x_max: bx_max,
				y_max: by_max,
			},
		))
	}

	/// Check whether this quadtree has any tiles in common with `other`.
	///
	/// # Errors
	/// Returns an error if the zoom levels don't match.
	pub fn intersects(&self, other: &TileQuadtree) -> Result<bool> {
		anyhow::ensure!(
			self.zoom == other.zoom,
			"Cannot intersect quadtrees with different zoom levels: {} vs {}",
			self.zoom,
			other.zoom
		);
		Ok(self.root.intersects(&other.root))
	}
}
