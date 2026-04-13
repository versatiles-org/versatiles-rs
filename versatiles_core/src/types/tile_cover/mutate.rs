//! Mutation methods for [`TileCover`].

use super::TileCover;
use crate::{TileBBox, TileCoord};
use anyhow::Result;

impl TileCover {
	/// Inserts a single tile coordinate into this cover.
	///
	/// If the coordinate is already covered (Bbox contains it), this is a no-op.
	/// Otherwise the cover is upgraded to a `Tree` for exact representation and
	/// the coordinate is inserted.
	///
	/// # Errors
	/// Returns an error if the coordinate's level does not match this cover's level.
	pub fn include_coord(&mut self, coord: &TileCoord) -> Result<()> {
		if let TileCover::Bbox(b) = self {
			if b.is_empty() {
				return b.include_coord(coord);
			}
			if b.includes_coord(coord) {
				return Ok(());
			}
			self.upgrade_to_tree();
		}
		self.as_tree_mut().include_coord(coord)
	}

	/// Inserts all tiles in `bbox` into this cover.
	///
	/// If the bbox is already fully covered (Bbox contains it), this is a no-op.
	/// Otherwise the cover is upgraded to a `Tree` for exact representation.
	///
	/// # Errors
	/// Returns an error if `bbox`'s level does not match this cover's level.
	pub fn include_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		if let TileCover::Bbox(b) = self {
			if b.is_empty() {
				return b.include_bbox(bbox);
			}
			if b.includes_bbox(bbox) {
				return Ok(());
			}
			self.upgrade_to_tree();
		}
		self.as_tree_mut().include_bbox(bbox)
	}

	/// Removes a single tile coordinate from this cover.
	///
	/// If the coordinate is not covered (Bbox does not contain it), this is a no-op.
	/// Otherwise the cover is upgraded to a `Tree` for exact subtraction.
	///
	/// # Errors
	/// Returns an error if the coordinate's level does not match this cover's level.
	pub fn remove_coord(&mut self, coord: &TileCoord) -> Result<()> {
		if let TileCover::Bbox(b) = self {
			if b.is_empty() || !b.includes_coord(coord) {
				return Ok(());
			}
			self.upgrade_to_tree();
		}
		self.as_tree_mut().remove_coord(coord)
	}

	/// Removes all tiles in `bbox` from this cover.
	///
	/// If there is no overlap (Bbox does not intersect it), this is a no-op.
	/// Otherwise the cover is upgraded to a `Tree` for exact subtraction.
	///
	/// # Errors
	/// Returns an error if `bbox`'s level does not match this cover's level.
	pub fn remove_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		if let TileCover::Bbox(b) = self {
			if b.is_empty() || !b.intersects_bbox(bbox) {
				return Ok(());
			}
			self.upgrade_to_tree();
		}
		self.as_tree_mut().remove_bbox(bbox)
	}
}
