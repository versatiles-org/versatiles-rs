//! Conversion methods for [`TileQuadtreePyramid`].

use super::TileQuadtreePyramid;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, PyramidInfo, TileBBoxPyramid};

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

impl PyramidInfo for TileQuadtreePyramid {
	fn get_geo_bbox(&self) -> Option<GeoBBox> {
		self.get_geo_bbox()
	}

	fn get_zoom_min(&self) -> Option<u8> {
		self.get_level_min()
	}

	fn get_zoom_max(&self) -> Option<u8> {
		self.get_level_max()
	}
}
