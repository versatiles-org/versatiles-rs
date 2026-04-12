//! Display, Debug, and PartialEq for [`TilePyramid`].

use super::TilePyramid;
use crate::MAX_ZOOM_LEVEL;
use std::fmt;

fn fmt_pyramid(pyramid: &TilePyramid, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	let parts: Vec<String> = pyramid
		.levels
		.iter()
		.filter(|c| !c.is_empty())
		.filter_map(|c| c.bounds().map(|bbox| format!("{bbox:?}")))
		.collect();
	write!(f, "[{}]", parts.join(", "))
}

impl fmt::Display for TilePyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt_pyramid(self, f)
	}
}

impl fmt::Debug for TilePyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt_pyramid(self, f)
	}
}

impl PartialEq for TilePyramid {
	fn eq(&self, other: &Self) -> bool {
		for level in 0..=MAX_ZOOM_LEVEL as usize {
			if self.levels[level] != other.levels[level] {
				return false;
			}
		}
		true
	}
}
