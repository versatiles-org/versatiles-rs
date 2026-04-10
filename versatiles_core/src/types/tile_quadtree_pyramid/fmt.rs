//! Display and Debug formatting for [`TileQuadtreePyramid`].

use super::TileQuadtreePyramid;
use crate::MAX_ZOOM_LEVEL;
use std::fmt;

/// Format the pyramid as a compact list of non-empty levels with their bounding boxes.
///
/// Example: `[0: [0,0,0,0] (1x1), 1: [0,0,1,1] (2x2)]`
fn fmt_pyramid(pyramid: &TileQuadtreePyramid, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	let parts: Vec<String> = pyramid
		.levels
		.iter()
		.filter(|qt| !qt.is_empty())
		.filter_map(|qt| qt.bounds().map(|bbox| format!("{bbox:?}")))
		.collect();
	write!(f, "[{}]", parts.join(", "))
}

impl fmt::Display for TileQuadtreePyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt_pyramid(self, f)
	}
}

impl fmt::Debug for TileQuadtreePyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt_pyramid(self, f)
	}
}

impl PartialEq for TileQuadtreePyramid {
	fn eq(&self, other: &Self) -> bool {
		for level in 0..=MAX_ZOOM_LEVEL as usize {
			if self.levels[level] != other.levels[level] {
				return false;
			}
		}
		true
	}
}
