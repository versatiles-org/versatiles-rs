//! Constructors for [`TileQuadtreePyramid`].

use super::TileQuadtreePyramid;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBoxPyramid, TileQuadtree};
use anyhow::Result;
use std::array::from_fn;

impl TileQuadtreePyramid {
	/// Creates a new `TileQuadtreePyramid` with empty coverage for all zoom levels.
	///
	/// # Returns
	///
	/// A `TileQuadtreePyramid` where each level is an empty quadtree.
	#[must_use]
	pub fn new_empty() -> Self {
		TileQuadtreePyramid {
			levels: from_fn(|z| TileQuadtree::new_empty(u8::try_from(z).expect("zoom level index exceeds u8::MAX"))),
		}
	}

	/// Creates a new `TileQuadtreePyramid` with full coverage for all zoom levels.
	///
	/// # Returns
	///
	/// A `TileQuadtreePyramid` where each level is a full quadtree.
	#[must_use]
	pub fn new_full() -> Self {
		TileQuadtreePyramid {
			levels: from_fn(|z| TileQuadtree::new_full(u8::try_from(z).expect("zoom level index exceeds u8::MAX"))),
		}
	}

	/// Constructs a new `TileQuadtreePyramid` from a geographic bounding box.
	///
	/// For each zoom level in `[zoom_min..=zoom_max]`, derives tile coverage from the
	/// geographic bounding box. Levels outside the given range remain empty.
	///
	/// # Arguments
	///
	/// * `zoom_min` - The smallest zoom level to include.
	/// * `zoom_max` - The largest zoom level to include.
	/// * `geo_bbox` - The geographical bounding box to use.
	///
	/// # Errors
	///
	/// Returns an error if the zoom levels exceed `MAX_ZOOM_LEVEL` or if the
	/// geographic coordinates are invalid.
	pub fn from_geo_bbox(zoom_min: u8, zoom_max: u8, geo_bbox: &GeoBBox) -> Result<Self> {
		let mut pyramid = TileQuadtreePyramid::new_empty();
		for z in zoom_min..=zoom_max {
			pyramid.levels[z as usize] = TileQuadtree::from_geo(z, geo_bbox)?;
		}
		Ok(pyramid)
	}

	/// Constructs a new `TileQuadtreePyramid` from a [`TileBBoxPyramid`].
	///
	/// Converts each level's rectangular bounding box into a quadtree representation.
	/// This is a lossless conversion in that all tiles covered by the source pyramid
	/// will also be covered by the resulting quadtree pyramid; however, non-rectangular
	/// coverage cannot be represented in a `TileBBoxPyramid` to begin with.
	///
	/// # Errors
	///
	/// Returns an error if conversion of any level fails.
	pub fn from_bbox_pyramid(pyramid: &TileBBoxPyramid) -> Result<Self> {
		let mut result = TileQuadtreePyramid::new_empty();
		for z in 0..=MAX_ZOOM_LEVEL {
			let bbox = pyramid.get_level_bbox(z);
			result.levels[z as usize] = TileQuadtree::from_bbox(bbox)?;
		}
		Ok(result)
	}
}

impl Default for TileQuadtreePyramid {
	/// Creates a new `TileQuadtreePyramid` with all levels empty.
	fn default() -> Self {
		Self::new_empty()
	}
}
