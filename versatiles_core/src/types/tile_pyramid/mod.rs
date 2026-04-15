//! A unified multi-zoom tile coverage pyramid.
//!
//! [`TilePyramid`] holds one [`TileCover`] per zoom level (0 through
//! [`MAX_ZOOM_LEVEL`](crate::MAX_ZOOM_LEVEL)).
//!
//! Each level defaults to `TileCover::Bbox(empty)`. Levels automatically
//! upgrade to `TileCover::Tree` when non-rectangular operations (e.g.
//! [`intersect_geo_bbox`](TilePyramid::intersect_geo_bbox)) are applied.

mod constructors;
mod fmt;
mod mutate;
mod queries;
#[cfg(test)]
mod tests;

use crate::{GeoBBox, MAX_ZOOM_LEVEL, PyramidInfo, TileCover};

/// A pyramid of tile covers across all zoom levels 0–[`MAX_ZOOM_LEVEL`](crate::MAX_ZOOM_LEVEL).
///
/// Each level stores a [`TileCover`], which is either a rectangular
/// [`TileBBox`](crate::TileBBox) or a [`TileQuadtree`](crate::TileQuadtree).
///
/// # Examples
/// ```rust
/// use versatiles_core::{TileBBox, TilePyramid};
///
/// let mut pyramid = TilePyramid::new_empty();
/// assert!(pyramid.is_empty());
///
/// let bbox = TileBBox::from_min_and_max(5, 3, 4, 10, 15).unwrap();
/// pyramid.insert_bbox(&bbox).unwrap();
/// assert!(!pyramid.is_empty());
/// assert_eq!(pyramid.level_min(), Some(5));
/// ```
#[derive(Clone)]
pub struct TilePyramid {
	levels: [TileCover; (MAX_ZOOM_LEVEL + 1) as usize],
}

impl PyramidInfo for TilePyramid {
	fn geo_bbox(&self) -> Option<GeoBBox> {
		TilePyramid::geo_bbox(self)
	}

	fn level_min(&self) -> Option<u8> {
		self.level_min()
	}

	fn level_max(&self) -> Option<u8> {
		self.level_max()
	}
}
