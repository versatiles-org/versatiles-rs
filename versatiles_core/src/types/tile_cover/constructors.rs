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
	/// Wraps `bbox` in the `Bbox` variant.
	fn from(bbox: TileBBox) -> Self {
		TileCover::Bbox(bbox)
	}
}

impl From<TileQuadtree> for TileCover {
	/// Wraps `tree` in the `Tree` variant.
	fn from(tree: TileQuadtree) -> Self {
		TileCover::Tree(tree)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn new_empty_is_empty() {
		let c = TileCover::new_empty(4).unwrap();
		assert!(c.is_empty());
		assert_eq!(c.level(), 4);
		assert_eq!(c.count_tiles(), 0);
	}

	#[test]
	fn new_full_covers_all() {
		let c = TileCover::new_full(2).unwrap();
		assert!(c.is_full());
		assert_eq!(c.count_tiles(), 16);
	}

	#[test]
	fn from_bbox_and_from_tree() {
		let b = bbox(3, 0, 0, 3, 3);
		let cb = TileCover::from(b);
		assert!(matches!(cb, TileCover::Bbox(_)));
		assert_eq!(cb.count_tiles(), 16);

		let t = TileQuadtree::new_full(3).unwrap();
		let ct = TileCover::from(t);
		assert!(matches!(ct, TileCover::Tree(_)));
		assert!(ct.is_full());
	}
}
