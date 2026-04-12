//! Query methods for [`TileCover`].

use super::TileCover;
use crate::{GeoBBox, TileBBox, TileCoord};
use anyhow::Result;

impl TileCover {
	/// Returns the zoom level of this cover.
	#[must_use]
	pub fn level(&self) -> u8 {
		match self {
			TileCover::Bbox(b) => b.level,
			TileCover::Tree(t) => t.zoom(),
		}
	}

	/// Returns `true` if this cover contains no tiles.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		match self {
			TileCover::Bbox(b) => b.is_empty(),
			TileCover::Tree(t) => t.is_empty(),
		}
	}

	/// Returns `true` if this cover contains all tiles at its zoom level.
	#[must_use]
	pub fn is_full(&self) -> bool {
		match self {
			TileCover::Bbox(b) => b.is_full(),
			TileCover::Tree(t) => t.is_full(),
		}
	}

	/// Returns the total number of tiles in this cover.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		match self {
			TileCover::Bbox(b) => b.count_tiles(),
			TileCover::Tree(t) => t.count_tiles(),
		}
	}

	/// Returns the tightest axis-aligned [`TileBBox`] containing all tiles,
	/// or `None` if this cover is empty.
	#[must_use]
	pub fn bounds(&self) -> Option<TileBBox> {
		match self {
			TileCover::Bbox(b) => {
				if b.is_empty() {
					None
				} else {
					Some(*b)
				}
			}
			TileCover::Tree(t) => t.bounds(),
		}
	}

	/// Converts the covered area to a geographic [`GeoBBox`], or `None` if empty.
	#[must_use]
	pub fn to_geo_bbox(&self) -> Option<GeoBBox> {
		match self {
			TileCover::Bbox(b) => b.to_geo_bbox(),
			TileCover::Tree(t) => t.to_geo_bbox(),
		}
	}

	/// Returns `true` if the given tile coordinate is contained in this cover.
	///
	/// # Errors
	/// Returns an error if the coordinate's level does not match this cover's level.
	pub fn includes_coord(&self, coord: TileCoord) -> Result<bool> {
		match self {
			TileCover::Bbox(b) => Ok(b.includes_coord(&coord)),
			TileCover::Tree(t) => t.includes_coord(coord),
		}
	}

	/// Returns `true` if all tiles in `bbox` are contained in this cover.
	///
	/// # Errors
	/// Returns an error if `bbox`'s level does not match this cover's level.
	pub fn includes_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		match self {
			TileCover::Bbox(b) => Ok(b.includes_bbox(bbox)),
			TileCover::Tree(t) => t.includes_bbox(bbox),
		}
	}

	/// Returns `true` if this cover overlaps the given `bbox`.
	///
	/// For the `Tree` variant this is an approximate check via [`bounds`](Self::bounds).
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		match self {
			TileCover::Bbox(b) => b.intersects_bbox(bbox),
			TileCover::Tree(t) => t.bounds().is_some_and(|b| b.intersects_bbox(bbox)),
		}
	}
}
