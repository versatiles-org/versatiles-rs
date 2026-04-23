//! Display, Debug, and PartialEq for [`TileCover`].

use super::TileCover;
use std::fmt;

impl fmt::Display for TileCover {
	/// Formats the cover using the inner `TileBBox` or `TileQuadtree` display.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			TileCover::Bbox(b) => write!(f, "{b}"),
			TileCover::Tree(t) => write!(f, "{t}"),
		}
	}
}

impl fmt::Debug for TileCover {
	/// Formats the cover with its variant tag for debugging.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			TileCover::Bbox(b) => write!(f, "TileCover::Bbox({b:?})"),
			TileCover::Tree(t) => write!(f, "TileCover::Tree({t})"),
		}
	}
}

impl PartialEq for TileCover {
	/// Two covers are equal when they represent the same set of tiles at the
	/// same zoom level.
	///
	/// Mixed variants (Bbox vs Tree) are compared by bounding box after a
	/// quick tile-count check.
	fn eq(&self, other: &Self) -> bool {
		if self.level() != other.level() {
			return false;
		}
		match (self, other) {
			(TileCover::Bbox(a), TileCover::Bbox(b)) => a == b,
			(TileCover::Tree(a), TileCover::Tree(b)) => a == b,
			_ => {
				if self.count_tiles() != other.count_tiles() {
					return false;
				}
				self.to_bbox() == other.to_bbox()
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{TileBBox, TileQuadtree};
	use rstest::rstest;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	/// `PartialEq` across variants at the same level.
	#[rstest]
	#[case::bbox_eq_bbox(TileCover::from(bbox(4, 1, 1, 5, 5)), TileCover::from(bbox(4, 1, 1, 5, 5)), true)]
	#[case::bbox_eq_tree_same_coverage(
		TileCover::from(bbox(3, 0, 0, 7, 7)),
		TileCover::from(TileQuadtree::from_bbox(&bbox(3, 0, 0, 7, 7))),
		true,
	)]
	#[case::different_levels(TileCover::new_empty(2).unwrap(), TileCover::new_empty(3).unwrap(), false)]
	fn eq_cases(#[case] a: TileCover, #[case] b: TileCover, #[case] expected_eq: bool) {
		assert_eq!(a == b, expected_eq);
	}

	/// Display output is non-empty on both variants; tree variant additionally
	/// shows its zoom level.
	#[rstest]
	#[case::bbox_variant(TileCover::from(bbox(3, 0, 0, 7, 7)), None)]
	#[case::tree_variant(TileCover::from(TileQuadtree::new_full(3).unwrap()), Some("zoom=3"))]
	fn display_cases(#[case] c: TileCover, #[case] must_contain: Option<&str>) {
		let s = format!("{c}");
		assert!(!s.is_empty());
		if let Some(substr) = must_contain {
			assert!(s.contains(substr), "expected {substr:?} in {s:?}");
		}
	}
}
