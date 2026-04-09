//! Conversion methods for [`TileQuadtreePyramid`].

use super::TileQuadtreePyramid;
use crate::{MAX_ZOOM_LEVEL, TileBBoxPyramid};

impl TileQuadtreePyramid {
	/// Converts this pyramid to a [`TileBBoxPyramid`] by computing the bounding box
	/// of each zoom level's quadtree.
	///
	/// This is a lossy conversion: the resulting pyramid uses rectangular bounding boxes
	/// and cannot represent non-rectangular coverage. It is primarily intended for
	/// backward compatibility with code that still requires rectangular bounds.
	///
	/// Levels with empty quadtrees produce empty bounding boxes in the output.
	#[must_use]
	pub fn to_bbox_pyramid(&self) -> TileBBoxPyramid {
		let mut pyramid = TileBBoxPyramid::new_empty();
		for z in 0..=MAX_ZOOM_LEVEL {
			if let Some(bbox) = self.levels[z as usize].bounds() {
				pyramid.set_level_bbox(bbox);
			}
		}
		pyramid
	}
}
