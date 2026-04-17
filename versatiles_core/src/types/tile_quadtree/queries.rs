//! Query methods for [`TileQuadtree`].

use super::constructors::{check_bbox_zoom, check_coord_zoom};
use super::{BBox, TileQuadtree};
use crate::types::tile_quadtree::node::Node;
use crate::{GeoBBox, TileBBox, TileCoord};
use anyhow::Result;
use versatiles_derive::context;

impl TileQuadtree {
	/// Return true if the quadtree contains no tiles.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.root.is_empty()
	}

	/// Return true if the quadtree contains all tiles at its zoom level.
	#[must_use]
	pub fn is_full(&self) -> bool {
		self.root.is_full()
	}

	/// Count the total number of tiles in the quadtree.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		self.root.count_tiles(self.level)
	}

	/// Count the number of internal (Partial) nodes in the quadtree.
	#[must_use]
	pub fn count_nodes(&self) -> u64 {
		self.root.count_nodes()
	}

	/// Return the tightest axis-aligned [`TileBBox`] containing all tiles,
	/// or `None` if the quadtree is empty.
	#[must_use]
	pub fn bbox(&self) -> Option<TileBBox> {
		let size = 1u64 << self.level;
		self.root.bounds((0, 0), size).map(|(x0, y0, x1, y1)| {
			TileBBox::from_min_and_max(
				self.level,
				u32::try_from(x0).unwrap(),
				u32::try_from(y0).unwrap(),
				u32::try_from(x1 - 1).unwrap(),
				u32::try_from(y1 - 1).unwrap(),
			)
			.unwrap()
		})
	}

	/// Convert the covered area to a geographic [`GeoBBox`], or `None` if empty.
	#[must_use]
	pub fn to_geo_bbox(&self) -> Option<GeoBBox> {
		self.bbox().map(|bb| bb.to_geo_bbox().unwrap())
	}

	/// Check whether a specific tile coordinate is in this quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's level doesn't match this quadtree's zoom.
	#[context("Failed to check TileCoord {coord:?} against TileQuadtree at level {}", self.level)]
	pub fn includes_coord(&self, coord: &TileCoord) -> Result<bool> {
		check_coord_zoom(coord, self.level)?;
		let size = 1u64 << self.level;
		Ok(self
			.root
			.includes_coord((0, 0), size, (u64::from(coord.x), u64::from(coord.y))))
	}

	/// Check whether all tiles in `bbox` are in this quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's level doesn't match this quadtree's zoom.
	#[context("Failed to check TileBBox {bbox:?} against TileQuadtree at level {}", self.level)]
	pub fn includes_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		check_bbox_zoom(bbox, self.level)?;
		let size = 1u64 << self.level;
		let Some(bbox) = BBox::new(bbox) else {
			return Ok(true);
		};
		Ok(self.root.includes_bbox(0, 0, size, bbox))
	}

	/// Check whether this quadtree has any tiles in common with `other`.
	///
	/// # Errors
	/// Returns an error if the zoom levels don't match.
	#[context("Failed to check intersection of TileQuadtrees at levels {} and {}", self.level, other.level)]
	pub fn intersects_tree(&self, other: &TileQuadtree) -> Result<bool> {
		anyhow::ensure!(
			self.level == other.level,
			"Cannot intersect quadtrees with different zoom levels: {} vs {}",
			self.level,
			other.level
		);
		Ok(self.root.intersects_tree(&other.root))
	}
}

impl Node {
	/// Return true if all tiles in this node's subtree are covered.
	#[must_use]
	pub(crate) fn is_full(&self) -> bool {
		matches!(self, Node::Full)
	}

	/// Return true if no tiles in this node's subtree are covered.
	#[must_use]
	pub(crate) fn is_empty(&self) -> bool {
		matches!(self, Node::Empty)
	}

	pub fn count_tiles(&self, remaining_depth: u8) -> u64 {
		match self {
			Node::Empty => 0,
			Node::Full => 1u64 << (2 * u32::from(remaining_depth)),
			Node::Partial(children) => {
				if remaining_depth == 0 {
					// Shouldn't happen in a well-formed tree, but handle gracefully
					1
				} else {
					children.iter().map(|c| c.count_tiles(remaining_depth - 1)).sum()
				}
			}
		}
	}

	pub fn count_nodes(&self) -> u64 {
		match self {
			Node::Empty | Node::Full => 1,
			Node::Partial(children) => 1 + children.iter().map(Node::count_nodes).sum::<u64>(),
		}
	}

	/// Returns the bounding box `(x_min, y_min, x_max_excl, y_max_excl)` of non-empty tiles.
	pub fn bounds(&self, (x_off, y_off): (u64, u64), size: u64) -> Option<(u64, u64, u64, u64)> {
		match self {
			Node::Empty => None,
			Node::Full => Some((x_off, y_off, x_off + size, y_off + size)),
			Node::Partial(children) => {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let child_offsets = [(x_off, y_off), (mid_x, y_off), (x_off, mid_y), (mid_x, mid_y)];
				let mut result: Option<(u64, u64, u64, u64)> = None;
				for (i, child) in children.iter().enumerate() {
					let (cx, cy) = child_offsets[i];
					if let Some(b) = child.bounds((cx, cy), half) {
						result = Some(match result {
							None => b,
							Some(r) => (r.0.min(b.0), r.1.min(b.1), r.2.max(b.2), r.3.max(b.3)),
						});
					}
				}
				result
			}
		}
	}

	pub fn includes_coord(&self, (x_off, y_off): (u64, u64), size: u64, (tx, ty): (u64, u64)) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let (idx, cx, cy, half) = Node::child_quadrant((x_off, y_off), size, (tx, ty));
				children[idx].includes_coord((cx, cy), half, (tx, ty))
			}
		}
	}

	pub fn includes_bbox(&self, x_off: u64, y_off: u64, size: u64, bbox: BBox) -> bool {
		match self {
			Node::Empty => false,
			Node::Full => true,
			Node::Partial(children) => {
				let half = size / 2;
				let mid_x = x_off + half;
				let mid_y = y_off + half;
				let child_offsets = [(x_off, y_off), (mid_x, y_off), (x_off, mid_y), (mid_x, mid_y)];
				for (i, child) in children.iter().enumerate() {
					let (cx, cy) = child_offsets[i];
					let cx_max = cx + half;
					let cy_max = cy + half;
					// Clip bbox against this child's region
					let ix_min = bbox.x_min.max(cx);
					let iy_min = bbox.y_min.max(cy);
					let ix_max = bbox.x_max.min(cx_max);
					let iy_max = bbox.y_max.min(cy_max);
					if ix_min < ix_max && iy_min < iy_max {
						// Pass the clipped sub-bbox so children don't re-clip unnecessarily
						let child_bbox = BBox {
							x_min: ix_min,
							y_min: iy_min,
							x_max: ix_max,
							y_max: iy_max,
						};
						if !child.includes_bbox(cx, cy, half, child_bbox) {
							return false;
						}
					}
				}
				true
			}
		}
	}

	pub fn intersects_tree(&self, b: &Node) -> bool {
		match (self, b) {
			(Node::Empty, _) | (_, Node::Empty) => false,
			(Node::Full, _) | (_, Node::Full) => true,
			(Node::Partial(ac), Node::Partial(bc)) => ac.iter().zip(bc.iter()).any(|(ac, bc)| ac.intersects_tree(bc)),
		}
	}
}
