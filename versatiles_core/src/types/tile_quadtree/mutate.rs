//! Mutation methods for [`TileQuadtree`].

use super::constructors::{check_bbox_zoom, check_coord_zoom};
use super::{BBox, TileQuadtree};
use crate::{TileBBox, TileCoord};
use anyhow::Result;

impl TileQuadtree {
	/// Insert a single tile into the quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's zoom level doesn't match.
	pub fn include_coord(&mut self, coord: &TileCoord) -> Result<()> {
		check_coord_zoom(coord, self.level)?;
		let size = 1u64 << self.level;
		self
			.root
			.insert_tile(0, 0, size, u64::from(coord.x), u64::from(coord.y));
		Ok(())
	}

	/// Insert all tiles within a [`TileBBox`] into the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	pub fn include_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.level)?;
		if bbox.is_empty() {
			return Ok(());
		}
		let size = 1u64 << self.level;
		let bx_min = u64::from(bbox.x_min()?);
		let by_min = u64::from(bbox.y_min()?);
		let bx_max = u64::from(bbox.x_max()?) + 1;
		let by_max = u64::from(bbox.y_max()?) + 1;
		self.root.include_bbox(
			0,
			0,
			size,
			BBox {
				x_min: bx_min,
				y_min: by_min,
				x_max: bx_max,
				y_max: by_max,
			},
		);
		Ok(())
	}

	/// Remove a single tile from the quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's zoom level doesn't match.
	pub fn remove_coord(&mut self, coord: &TileCoord) -> Result<()> {
		check_coord_zoom(coord, self.level)?;
		let size = 1u64 << self.level;
		self
			.root
			.remove_tile(0, 0, size, u64::from(coord.x), u64::from(coord.y));
		Ok(())
	}

	/// Remove all tiles within a [`TileBBox`] from the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	pub fn remove_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.level)?;
		if bbox.is_empty() {
			return Ok(());
		}
		let size = 1u64 << self.level;
		let x_min = u64::from(bbox.x_min()?);
		let y_min = u64::from(bbox.y_min()?);
		let x_max = u64::from(bbox.x_max()?) + 1;
		let y_max = u64::from(bbox.y_max()?) + 1;
		self.root.remove_bbox(
			0,
			0,
			size,
			BBox {
				x_min,
				y_min,
				x_max,
				y_max,
			},
		);
		Ok(())
	}
}
