//! Constructors for [`TileCover`].

use super::TileCover;
use crate::{GeoBBox, TileBBox, TileQuadtree};
use anyhow::Result;
use versatiles_derive::context;

impl TileCover {
	/// Creates an empty `TileCover` (Bbox variant) at the given zoom level.
	///
	/// # Example
	/// ```
	/// use versatiles_core::TileCover;
	/// let c = TileCover::new_empty(5).unwrap();
	/// assert!(c.is_empty());
	/// assert_eq!(c.level(), 5);
	/// ```
	#[context("Failed to create empty TileCover at level {level}")]
	pub fn new_empty(level: u8) -> Result<Self> {
		Ok(TileCover::Bbox(TileBBox::new_empty(level)?))
	}

	/// Creates a full `TileCover` (Bbox variant) at the given zoom level.
	///
	/// # Example
	/// ```
	/// use versatiles_core::TileCover;
	/// let c = TileCover::new_full(2).unwrap();
	/// assert!(c.is_full());
	/// assert_eq!(c.count_tiles(), 16);
	/// ```
	#[context("Failed to create full TileCover at level {level}")]
	pub fn new_full(level: u8) -> Result<Self> {
		Ok(TileCover::Bbox(TileBBox::new_full(level)?))
	}

	/// Creates a `TileCover` from a geographic bounding box at the given zoom level.
	///
	/// # Errors
	/// Returns an error if the level or geographic coordinates are invalid.
	#[context("Failed to create TileCover from GeoBBox {bbox:?} at level {level}")]
	pub fn from_geo_bbox(level: u8, bbox: &GeoBBox) -> Result<Self> {
		Ok(TileCover::Bbox(TileBBox::from_geo_bbox(level, bbox)?))
	}
}

impl From<TileBBox> for TileCover {
	fn from(bbox: TileBBox) -> Self {
		TileCover::Bbox(bbox)
	}
}

impl From<TileQuadtree> for TileCover {
	fn from(tree: TileQuadtree) -> Self {
		TileCover::Tree(tree)
	}
}
