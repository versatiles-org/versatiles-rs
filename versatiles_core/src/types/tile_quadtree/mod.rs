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

	/// Root cell of a quadtree at `level`: `[0, 2^level) × [0, 2^level)`.
	fn root(level: u8) -> Self {
		let size = 1u64 << level;
		Self::new(0, 0, size, size)
	}

	fn from_bbox(bbox: &TileBBox) -> Option<Self> {
		if bbox.is_empty() {
			return None;
		}
		Some(Self {
			x_min: u64::from(bbox.x_min().expect("bbox is non-empty")),
			y_min: u64::from(bbox.y_min().expect("bbox is non-empty")),
			x_max: u64::from(bbox.x_max().expect("bbox is non-empty")) + 1,
			y_max: u64::from(bbox.y_max().expect("bbox is non-empty")) + 1,
		})
	}

	fn into_bbox(self, level: u8) -> TileBBox {
		TileBBox::from_min_and_max(
			level,
			u32::try_from(self.x_min).expect("within level bounds"),
			u32::try_from(self.y_min).expect("within level bounds"),
			u32::try_from(self.x_max - 1).expect("within level bounds"),
			u32::try_from(self.y_max - 1).expect("within level bounds"),
		)
		.expect("bbox valid at level")
	}

	/// Side length. Quadtree cells are always square.
	fn size(&self) -> u64 {
		debug_assert_eq!(self.x_max - self.x_min, self.y_max - self.y_min);
		self.x_max - self.x_min
	}

	/// Split a square cell into `[NW, NE, SW, SE]` quadrants.
	///
	/// Side length must be ≥ 2 and even (debug-checked). Quadtree cells at
	/// `size > 1` always satisfy this since every recursion halves the side.
	fn quadrants(&self) -> [BBox; 4] {
		let half = self.size() / 2;
		debug_assert!(half > 0, "cannot split a 1×1 cell");
		let (x0, y0) = (self.x_min, self.y_min);
		let (mx, my) = (x0 + half, y0 + half);
		[
			BBox::new(x0, y0, mx, my),               // NW
			BBox::new(mx, y0, mx + half, my),        // NE
			BBox::new(x0, my, mx, my + half),        // SW
			BBox::new(mx, my, mx + half, my + half), // SE
		]
	}

	/// Clip `self` to `other`. Returns `None` if the result is empty.
	fn intersection(&self, other: &BBox) -> Option<BBox> {
		let x_min = self.x_min.max(other.x_min);
		let y_min = self.y_min.max(other.y_min);
		let x_max = self.x_max.min(other.x_max);
		let y_max = self.y_max.min(other.y_max);
		(x_min < x_max && y_min < y_max).then(|| BBox::new(x_min, y_min, x_max, y_max))
	}

	/// Returns `true` if `self` fully contains `other`.
	fn covers(&self, other: &BBox) -> bool {
		self.x_min <= other.x_min && self.y_min <= other.y_min && self.x_max >= other.x_max && self.y_max >= other.y_max
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
