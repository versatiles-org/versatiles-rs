//! A quadtree-based tile set for efficient spatial operations.
//!
//! [`TileQuadtree`] represents a set of tiles at a fixed zoom level using a
//! quadtree structure that compresses uniform regions to single nodes.
//!
//! # Node convention
//! Each `Partial` node stores four children in `[NW, NE, SW, SE]` order:
//! - Index 0 = NW (x < mid, y < mid)
//! - Index 1 = NE (x >= mid, y < mid)
//! - Index 2 = SW (x < mid, y >= mid)
//! - Index 3 = SE (x >= mid, y >= mid)

mod constructors;
mod convert;
mod fmt;
mod include;
mod info_trait;
mod intersect;
mod iter;
mod mutate;
mod node;
mod queries;
mod serialize;
mod set_ops;
mod zoom;

use node::Node;

use crate::TileBBox;

/// A compact axis-aligned bounding box used internally by recursive quadtree helpers.
///
/// All coordinates are in tile-space and the max values are exclusive.
#[derive(Clone, Copy)]
pub(super) struct BBox {
	pub x_min: u64,
	pub y_min: u64,
	pub x_max: u64,
	pub y_max: u64,
}

impl BBox {
	fn new(x_min: u64, y_min: u64, x_max: u64, y_max: u64) -> Self {
		Self {
			x_min,
			y_min,
			x_max,
			y_max,
		}
	}
	fn from_bbox(bbox: &TileBBox) -> Option<Self> {
		if bbox.is_empty() {
			return None;
		}
		Some(Self {
			x_min: u64::from(bbox.x_min().unwrap()),
			y_min: u64::from(bbox.y_min().unwrap()),
			x_max: u64::from(bbox.x_max().unwrap()) + 1,
			y_max: u64::from(bbox.y_max().unwrap()) + 1,
		})
	}
	fn into_bbox(self, level: u8) -> TileBBox {
		TileBBox::from_min_and_max(
			level,
			u32::try_from(self.x_min).unwrap(),
			u32::try_from(self.y_min).unwrap(),
			u32::try_from(self.x_max - 1).unwrap(),
			u32::try_from(self.y_max - 1).unwrap(),
		)
		.unwrap()
	}
	fn union(mut self, other: Self) -> Self {
		self.x_min = self.x_min.min(other.x_min);
		self.y_min = self.y_min.min(other.y_min);
		self.x_max = self.x_max.max(other.x_max);
		self.y_max = self.y_max.max(other.y_max);
		self
	}
}

/// A set of tiles at a single zoom level, backed by a quadtree.
///
/// # Examples
/// ```
/// use versatiles_core::TileQuadtree;
///
/// let tree = TileQuadtree::new_empty(5).unwrap();
/// assert!(tree.is_empty());
///
/// let full = TileQuadtree::new_full(3).unwrap();
/// assert!(full.is_full());
/// assert_eq!(full.count_tiles(), 64); // 8×8 tiles at zoom 3
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TileQuadtree {
	level: u8,
	root: Node,
}

impl TileQuadtree {
	/// Return the zoom level of this quadtree.
	#[must_use]
	pub fn level(&self) -> u8 {
		self.level
	}
}
