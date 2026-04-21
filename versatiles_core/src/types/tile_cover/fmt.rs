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

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn eq_bbox_bbox() {
		let a = TileCover::from(bbox(4, 1, 1, 5, 5));
		let b = TileCover::from(bbox(4, 1, 1, 5, 5));
		assert_eq!(a, b);
	}

	#[test]
	fn eq_bbox_tree_same_coverage() {
		let b = bbox(3, 0, 0, 7, 7);
		let cb = TileCover::from(b);
		let ct = TileCover::from(TileQuadtree::from_bbox(&b));
		assert_eq!(cb, ct);
	}

	#[test]
	fn neq_different_levels() {
		let a = TileCover::new_empty(2).unwrap();
		let b = TileCover::new_empty(3).unwrap();
		assert_ne!(a, b);
	}

	#[test]
	fn display_bbox_variant() {
		let c = TileCover::from(bbox(3, 0, 0, 7, 7));
		let s = format!("{c}");
		assert!(!s.is_empty());
	}

	#[test]
	fn display_tree_variant() {
		let c = TileCover::from(TileQuadtree::new_full(3).unwrap());
		let s = format!("{c}");
		assert!(s.contains("zoom=3"));
	}
}
