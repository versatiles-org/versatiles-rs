use crate::{MAX_ZOOM_LEVEL, TileBBoxPyramid};
use std::fmt;

impl fmt::Debug for TileBBoxPyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Debug: show only non-empty levels
		f.debug_list().entries(self.iter_levels()).finish()
	}
}

impl fmt::Display for TileBBoxPyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Display: also show only non-empty levels
		f.debug_list().entries(self.iter_levels()).finish()
	}
}

impl PartialEq for TileBBoxPyramid {
	fn eq(&self, other: &Self) -> bool {
		for level in 0..MAX_ZOOM_LEVEL {
			let bbox0 = self.get_level_bbox(level);
			let bbox1 = other.get_level_bbox(level);
			// If one is empty and the other is not, they're not equal
			if bbox0.is_empty() != bbox1.is_empty() {
				return false;
			}
			// If both are empty, skip
			if bbox0.is_empty() {
				continue;
			}
			// Otherwise, compare
			if bbox0 != bbox1 {
				return false;
			}
		}
		true
	}
}
