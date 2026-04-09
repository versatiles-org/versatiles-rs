//! Display and Debug formatting for [`TileQuadtreePyramid`].

use super::TileQuadtreePyramid;
use crate::MAX_ZOOM_LEVEL;
use std::fmt;

impl fmt::Display for TileQuadtreePyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let non_empty: Vec<_> = self.levels.iter().filter(|qt| !qt.is_empty()).collect();
		f.debug_list().entries(non_empty.iter()).finish()
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
