//! Mutation methods for [`TileCover`].

use super::TileCover;
use crate::{TileBBox, TileCoord};
use anyhow::Result;

impl TileCover {
	/// Inserts a single tile coordinate into this cover.
	///
	/// For the `Bbox` variant the bounding rectangle is expanded to include the
	/// coordinate (same semantics as [`TileBBox::include_coord`]).
	///
	/// # Errors
	/// Returns an error if the coordinate's level does not match this cover's level.
	pub fn include_coord(&mut self, coord: TileCoord) -> Result<()> {
		match self {
			TileCover::Bbox(b) => {
				if b.includes_coord(&coord) {
					Ok(())
				} else {
					self.upgrade_to_tree();
					match self {
						TileCover::Tree(t) => t.include_coord(coord),
						TileCover::Bbox(_) => unreachable!(),
					}
				}
			}
			TileCover::Tree(t) => t.include_coord(coord),
		}
	}

	/// Inserts all tiles in `bbox` into this cover.
	///
	/// For the `Bbox` variant the bounding rectangle is expanded (same semantics
	/// as [`TileBBox::include_bbox`]).
	///
	/// # Errors
	/// Returns an error if `bbox`'s level does not match this cover's level.
	pub fn include_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		match self {
			TileCover::Bbox(b) => {
				if b.includes_bbox(bbox) {
					Ok(())
				} else {
					self.upgrade_to_tree();
					match self {
						TileCover::Tree(t) => t.include_bbox(bbox),
						TileCover::Bbox(_) => unreachable!(),
					}
				}
			}
			TileCover::Tree(t) => t.include_bbox(bbox),
		}
	}

	/// Removes a single tile coordinate from this cover.
	///
	/// If this cover is the `Bbox` variant, it is first converted to a `Tree`
	/// to allow exact subtraction.
	///
	/// # Errors
	/// Returns an error if the coordinate's level does not match this cover's level.
	pub fn remove_coord(&mut self, coord: TileCoord) -> Result<()> {
		match self {
			TileCover::Tree(t) => t.remove_coord(coord),
			TileCover::Bbox(b) => {
				if b.includes_coord(&coord) {
					self.upgrade_to_tree();
					match self {
						TileCover::Tree(t) => t.remove_coord(coord),
						TileCover::Bbox(_) => unreachable!(),
					}
				} else {
					Ok(())
				}
			}
		}
	}

	/// Removes all tiles in `bbox` from this cover.
	///
	/// If this cover is the `Bbox` variant, it is first converted to a `Tree`
	/// to allow exact subtraction.
	///
	/// # Errors
	/// Returns an error if `bbox`'s level does not match this cover's level.
	pub fn remove_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		match self {
			TileCover::Tree(t) => t.remove_bbox(bbox),
			TileCover::Bbox(b) => {
				if b.intersects_bbox(bbox) {
					self.upgrade_to_tree();
					match self {
						TileCover::Tree(t) => t.remove_bbox(bbox),
						TileCover::Bbox(_) => unreachable!(),
					}
				} else {
					Ok(())
				}
			}
		}
	}
}
