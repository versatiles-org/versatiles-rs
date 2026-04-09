//! Query methods for [`TileQuadtree`].

use super::constructors::{check_bbox_zoom, check_coord_zoom};
use super::{BBox, Node, TileQuadtree};
use crate::{GeoBBox, TileBBox, TileCoord};
use anyhow::Result;

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
	///
	/// # Examples
	/// ```
	/// use versatiles_core::TileQuadtree;
	/// assert_eq!(TileQuadtree::new_full(2).tile_count(), 16);
	/// assert_eq!(TileQuadtree::new_empty(2).tile_count(), 0);
	/// ```
	#[must_use]
	pub fn tile_count(&self) -> u64 {
		node_count(&self.root, self.zoom)
	}

	/// Return the tightest axis-aligned [`TileBBox`] containing all tiles,
	/// or `None` if the quadtree is empty.
	#[must_use]
	pub fn bounds(&self) -> Option<TileBBox> {
		let size = 1u64 << self.zoom;
		node_bounds(&self.root, 0, 0, size).map(|(x0, y0, x1, y1)| {
			TileBBox::from_min_and_max(
				self.zoom,
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
		self.bounds().map(|bb| bb.to_geo_bbox().unwrap())
	}

	/// Check whether a specific tile coordinate is in this quadtree.
	///
	/// # Errors
	/// Returns an error if the coordinate's level doesn't match this quadtree's zoom.
	pub fn contains_tile(&self, coord: TileCoord) -> Result<bool> {
		check_coord_zoom(coord, self.zoom)?;
		let size = 1u64 << self.zoom;
		Ok(node_contains_tile(
			&self.root,
			0,
			0,
			size,
			u64::from(coord.x),
			u64::from(coord.y),
		))
	}

	/// Check whether all tiles in `bbox` are in this quadtree.
	///
	/// # Errors
	/// Returns an error if the bbox's level doesn't match this quadtree's zoom.
	pub fn contains_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		check_bbox_zoom(bbox, self.zoom)?;
		if bbox.is_empty() {
			return Ok(true);
		}
		let size = 1u64 << self.zoom;
		let bx_min = u64::from(bbox.x_min()?);
		let by_min = u64::from(bbox.y_min()?);
		let bx_max = u64::from(bbox.x_max()?) + 1;
		let by_max = u64::from(bbox.y_max()?) + 1;
		Ok(node_contains_bbox(
			&self.root,
			0,
			0,
			size,
			BBox {
				x_min: bx_min,
				y_min: by_min,
				x_max: bx_max,
				y_max: by_max,
			},
		))
	}

	/// Check whether this quadtree has any tiles in common with `other`.
	///
	/// # Errors
	/// Returns an error if the zoom levels don't match.
	pub fn intersects(&self, other: &TileQuadtree) -> Result<bool> {
		anyhow::ensure!(
			self.zoom == other.zoom,
			"Cannot intersect quadtrees with different zoom levels: {} vs {}",
			self.zoom,
			other.zoom
		);
		Ok(node_intersects(&self.root, &other.root))
	}
}

fn node_count(node: &Node, remaining_depth: u8) -> u64 {
	match node {
		Node::Empty => 0,
		Node::Full => 1u64 << (2 * u32::from(remaining_depth)),
		Node::Partial(children) => {
			if remaining_depth == 0 {
				// Shouldn't happen in a well-formed tree, but handle gracefully
				1
			} else {
				children.iter().map(|c| node_count(c, remaining_depth - 1)).sum()
			}
		}
	}
}

/// Returns the bounding box `(x_min, y_min, x_max_excl, y_max_excl)` of non-empty tiles.
fn node_bounds(node: &Node, x_off: u64, y_off: u64, size: u64) -> Option<(u64, u64, u64, u64)> {
	match node {
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
				if let Some(b) = node_bounds(child, cx, cy, half) {
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

fn node_contains_tile(node: &Node, x_off: u64, y_off: u64, size: u64, tx: u64, ty: u64) -> bool {
	match node {
		Node::Empty => false,
		Node::Full => true,
		Node::Partial(children) => {
			let half = size / 2;
			let mid_x = x_off + half;
			let mid_y = y_off + half;
			let (idx, cx, cy) = if tx < mid_x {
				if ty < mid_y {
					(0, x_off, y_off)
				} else {
					(2, x_off, mid_y)
				}
			} else {
				if ty < mid_y {
					(1, mid_x, y_off)
				} else {
					(3, mid_x, mid_y)
				}
			};
			node_contains_tile(&children[idx], cx, cy, half, tx, ty)
		}
	}
}

fn node_contains_bbox(node: &Node, x_off: u64, y_off: u64, size: u64, bbox: BBox) -> bool {
	match node {
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
				// Check if bbox intersects this child's region
				let ix_min = bbox.x_min.max(cx);
				let iy_min = bbox.y_min.max(cy);
				let ix_max = bbox.x_max.min(cx_max);
				let iy_max = bbox.y_max.min(cy_max);
				if ix_min < ix_max && iy_min < iy_max {
					// bbox intersects this child — child must fully contain that intersection
					if !node_contains_bbox(child, cx, cy, half, bbox) {
						return false;
					}
				}
			}
			true
		}
	}
}

fn node_intersects(a: &Node, b: &Node) -> bool {
	match (a, b) {
		(Node::Empty, _) | (_, Node::Empty) => false,
		(Node::Full, _) | (_, Node::Full) => true,
		(Node::Partial(ac), Node::Partial(bc)) => ac.iter().zip(bc.iter()).any(|(ac, bc)| node_intersects(ac, bc)),
	}
}
