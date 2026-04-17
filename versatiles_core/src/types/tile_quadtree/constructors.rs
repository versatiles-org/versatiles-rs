//! Constructors for [`TileQuadtree`].

use super::{BBox, Node, TileQuadtree};
use crate::{GeoBBox, TileBBox, TileCoord, validate_zoom_level};
use anyhow::{Result, ensure};
use versatiles_derive::context;

impl TileQuadtree {
	/// Create an empty quadtree at the given zoom level.
	#[context("Failed to create empty TileQuadtree at level {level}")]
	pub fn new_empty(level: u8) -> Result<Self> {
		validate_zoom_level(level)?;
		Ok(TileQuadtree {
			level,
			root: Node::Empty,
		})
	}

	/// Create a full quadtree (all tiles covered) at the given zoom level.
	#[context("Failed to create full TileQuadtree at level {level}")]
	pub fn new_full(level: u8) -> Result<Self> {
		validate_zoom_level(level)?;
		Ok(TileQuadtree {
			level,
			root: Node::Full,
		})
	}

	/// Build a quadtree from a [`TileBBox`], covering exactly those tiles.
	///
	/// # Errors
	/// Returns an error if the bbox zoom level exceeds `MAX_ZOOM_LEVEL`.
	#[must_use]
	pub fn from_bbox(bbox: &TileBBox) -> Self {
		let level = bbox.level();
		validate_zoom_level(level).expect("TileBBox level should have been validated on construction");

		let Some(bbox) = BBox::new(bbox) else {
			return TileQuadtree::new_empty(level).unwrap();
		};
		let size = 1u64 << level;

		/// Recursively build a quadtree node for the cell covering
		/// `[x_off, x_off+size) × [y_off, y_off+size)` against the bbox
		/// `[bbox.x_min, bbox.x_max) × [bbox.y_min, bbox.y_max)` (all exclusive on max side).
		fn build_node(depth: u8, (x_off, y_off): (u64, u64), size: u64, bbox: &BBox) -> Node {
			// Intersection of bbox with this cell
			let ix_min = bbox.x_min.max(x_off);
			let iy_min = bbox.y_min.max(y_off);
			let ix_max = bbox.x_max.min(x_off + size);
			let iy_max = bbox.y_max.min(y_off + size);

			if ix_min >= ix_max || iy_min >= iy_max {
				return Node::Empty;
			}

			if ix_min == x_off && iy_min == y_off && ix_max == x_off + size && iy_max == y_off + size {
				return Node::Full;
			}

			if depth == 0 {
				// We've reached leaf level — any intersection means Full
				return Node::Full;
			}

			let half = size / 2;
			let mid_x = x_off + half;
			let mid_y = y_off + half;
			Node::new_partial([
				build_node(depth - 1, (x_off, y_off), half, bbox),
				build_node(depth - 1, (mid_x, y_off), half, bbox),
				build_node(depth - 1, (x_off, mid_y), half, bbox),
				build_node(depth - 1, (mid_x, mid_y), half, bbox),
			])
		}

		let root = build_node(level, (0, 0), size, &bbox);
		TileQuadtree { level, root }
	}

	/// Build a quadtree from a geographic bounding box at the given zoom level.
	///
	/// # Errors
	/// Returns an error if the zoom level or geographic coordinates are invalid.
	#[context("Failed to create TileQuadtree from GeoBBox {bbox:?} at level {level}")]
	pub fn from_geo_bbox(level: u8, bbox: &GeoBBox) -> Result<Self> {
		validate_zoom_level(level)?;
		let tile_bbox = TileBBox::from_geo_bbox(level, bbox)?;
		Ok(Self::from_bbox(&tile_bbox))
	}

	/// Build a quadtree from a `Vec` of `(x, y)` tile coordinates.
	///
	/// This is the efficient batch constructor for large tile sets:
	/// 1. Each tile's Morton (Z-order) code is computed once — O(T).
	/// 2. The codes are sorted with `sort_unstable` (pdqsort) — O(T log T).
	/// 3. The quadtree is built top-down using `partition_point` binary search
	///    on the sorted codes — O(T · level), with each split being a trivial
	///    `u64 <` comparison rather than a Morton recomputation.
	///
	/// Total extra allocation: one `Vec<u64>` of T × 8 bytes.
	///
	/// # Errors
	/// Returns an error if `level` exceeds `MAX_ZOOM_LEVEL`.
	#[context("Failed to build TileQuadtree from tile iterator at level {level}")]
	pub fn from_tile_coords(level: u8, tiles: &[(u32, u32)]) -> Result<Self> {
		validate_zoom_level(level)?;
		let mut codes: Vec<u64> = tiles.iter().map(|&(x, y)| morton(u64::from(x), u64::from(y))).collect();
		codes.sort_unstable();
		codes.dedup();
		let size = 1u64 << level;
		let root = Node::from_morton_sorted(&codes, 0, 0, size);
		Ok(TileQuadtree { level, root })
	}
}

/// Validate that a TileCoord belongs to the given zoom level.
pub(crate) fn check_coord_zoom(coord: &TileCoord, zoom: u8) -> Result<()> {
	ensure!(
		coord.level == zoom,
		"TileCoord level {} does not match quadtree zoom {}",
		coord.level,
		zoom
	);
	Ok(())
}

/// Validate that a TileBBox belongs to the given zoom level.
pub(crate) fn check_bbox_zoom(bbox: &TileBBox, zoom: u8) -> Result<()> {
	ensure!(
		bbox.level() == zoom,
		"TileBBox level {} does not match quadtree zoom {}",
		bbox.level(),
		zoom
	);
	Ok(())
}

impl Node {
	/// Build a `Node` from a sorted, deduplicated slice of Morton codes.
	///
	/// The slice must only contain codes for tiles within the cell
	/// `[x_off, x_off+size) × [y_off, y_off+size)` and be sorted ascending.
	/// Each quadrant split uses `partition_point` with a trivial `u64 <`
	/// comparison — no Morton recomputation per tile.
	pub(super) fn from_morton_sorted(codes: &[u64], x_off: u64, y_off: u64, size: u64) -> Node {
		if codes.is_empty() {
			return Node::Empty;
		}
		if codes.len() as u64 == size * size {
			return Node::Full;
		}
		if size == 1 {
			return Node::Full;
		}
		let half = size / 2;
		let mid_x = x_off + half;
		let mid_y = y_off + half;
		// Morton code of the first tile in each quadrant.
		let ne_base = morton(mid_x, y_off);
		let sw_base = morton(x_off, mid_y);
		let se_base = morton(mid_x, mid_y);

		let ne_start = codes.partition_point(|&c| c < ne_base);
		let sw_start = codes.partition_point(|&c| c < sw_base);
		let se_start = codes.partition_point(|&c| c < se_base);

		Node::new_partial([
			Node::from_morton_sorted(&codes[..ne_start], x_off, y_off, half), // NW
			Node::from_morton_sorted(&codes[ne_start..sw_start], mid_x, y_off, half), // NE
			Node::from_morton_sorted(&codes[sw_start..se_start], x_off, mid_y, half), // SW
			Node::from_morton_sorted(&codes[se_start..], mid_x, mid_y, half), // SE
		])
	}
}

/// Spread the bits of `n` by inserting a zero between every two bits.
///
/// Used to interleave x and y coordinates for Morton (Z-order) encoding.
/// Handles up to 32-bit inputs (zoom ≤ 31).
#[inline]
fn spread_bits(mut n: u64) -> u64 {
	n = (n | (n << 16)) & 0x0000_FFFF_0000_FFFF;
	n = (n | (n << 8)) & 0x00FF_00FF_00FF_00FF;
	n = (n | (n << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
	n = (n | (n << 2)) & 0x3333_3333_3333_3333;
	n = (n | (n << 1)) & 0x5555_5555_5555_5555;
	n
}

/// Compute the Morton (Z-order) code for tile `(x, y)`.
///
/// Interleaves x bits (even positions) with y bits (odd positions), producing
/// a single `u64` such that tiles within any power-of-2 aligned quadtree cell
/// form a contiguous range when the codes are sorted.
#[inline]
fn morton(x: u64, y: u64) -> u64 {
	spread_bits(x) | (spread_bits(y) << 1)
}
