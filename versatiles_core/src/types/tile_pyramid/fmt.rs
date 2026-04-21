//! Display, Debug, and PartialEq for [`TilePyramid`].

use super::TilePyramid;
use crate::MAX_ZOOM_LEVEL;
use std::fmt;

/// Shared formatting helper: lists non-empty levels separated by commas.
fn fmt_pyramid(pyramid: &TilePyramid, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	let parts: Vec<String> = pyramid
		.levels
		.iter()
		.filter(|c| !c.is_empty())
		.map(|c| format!("{c:?}"))
		.collect();
	write!(f, "[{}]", parts.join(", "))
}

impl fmt::Display for TilePyramid {
	/// Formats the pyramid as `[level, level, …]`, omitting empty levels.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt_pyramid(self, f)
	}
}

impl fmt::Debug for TilePyramid {
	/// Formats the pyramid with the same representation as `Display`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt_pyramid(self, f)
	}
}

impl PartialEq for TilePyramid {
	/// Two pyramids are equal when every zoom level has identical coverage.
	fn eq(&self, other: &Self) -> bool {
		for level in 0..=MAX_ZOOM_LEVEL as usize {
			if self.levels[level] != other.levels[level] {
				return false;
			}
		}
		true
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileBBox;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn display_empty_pyramid() {
		let p = TilePyramid::new_empty();
		assert_eq!(format!("{p}"), "[]");
	}

	#[test]
	fn display_nonempty_pyramid() {
		let mut p = TilePyramid::new_empty();
		p.set_level_bbox(bbox(3, 0, 0, 3, 3));
		let s = format!("{p}");
		// TileBBox Debug format: "3: [0,0,3,3] (4x4)"
		assert!(s.contains("3:"), "expected level 3 in pyramid display, got: {s}");
	}

	#[test]
	fn eq_empty_pyramids() {
		assert_eq!(TilePyramid::new_empty(), TilePyramid::new_empty());
	}

	#[test]
	fn eq_after_same_operations() {
		let mut a = TilePyramid::new_empty();
		a.insert_bbox(&bbox(5, 3, 4, 10, 15)).unwrap();

		let mut b = TilePyramid::new_empty();
		b.insert_bbox(&bbox(5, 3, 4, 10, 15)).unwrap();

		assert_eq!(a, b);
	}
}
