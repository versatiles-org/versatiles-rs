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
			.insert_coord((0, 0), size, (u64::from(coord.x), u64::from(coord.y)));
		Ok(())
	}

	/// Insert all tiles within a [`TileBBox`] into the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	pub fn include_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.level)?;
		let size = 1u64 << self.level;
		let Some(bbox) = BBox::new(bbox) else {
			return Ok(());
		};
		self.root.include_bbox((0, 0), size, &bbox);
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
			.remove_coord((0, 0), size, (u64::from(coord.x), u64::from(coord.y)));
		Ok(())
	}

	/// Remove all tiles within a [`TileBBox`] from the quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's zoom level doesn't match.
	pub fn remove_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		check_bbox_zoom(bbox, self.level)?;
		let Some(bbox) = BBox::new(bbox) else {
			return Ok(());
		};
		self.root.remove_bbox((0, 0), 1u64 << self.level, &bbox);
		Ok(())
	}
}
