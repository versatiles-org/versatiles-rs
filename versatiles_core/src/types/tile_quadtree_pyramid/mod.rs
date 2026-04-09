//! A pyramid of quadtree-based tile sets across multiple zoom levels.
//!
//! [`TileQuadtreePyramid`] holds one [`TileQuadtree`] per zoom level (0 through
//! `MAX_ZOOM_LEVEL`), forming a hierarchical description of tile coverage using
//! quadtree structures rather than rectangular bounding boxes.

mod constructors;
mod convert;
mod fmt;
mod mutate;
mod queries;
#[cfg(test)]
mod tests;

use crate::{MAX_ZOOM_LEVEL, TileQuadtree};

/// A pyramid of quadtree-based tile sets across multiple zoom levels.
///
/// Each level (`0` through `MAX_ZOOM_LEVEL`) corresponds to a [`TileQuadtree`], which
/// efficiently represents arbitrary tile coverage at that zoom level. Unlike
/// [`TileBBoxPyramid`](crate::TileBBoxPyramid), this structure can represent non-rectangular
/// coverage regions.
///
/// # Examples
///
/// ```rust
/// use versatiles_core::TileQuadtreePyramid;
///
/// let pyramid = TileQuadtreePyramid::new_empty();
/// assert!(pyramid.is_empty());
///
/// let full = TileQuadtreePyramid::new_full();
/// assert!(!full.is_empty());
/// assert_eq!(full.get_zoom_min(), Some(0));
/// assert_eq!(full.get_zoom_max(), Some(30));
/// ```
#[derive(Clone, Debug)]
pub struct TileQuadtreePyramid {
	levels: [TileQuadtree; (MAX_ZOOM_LEVEL + 1) as usize],
}
