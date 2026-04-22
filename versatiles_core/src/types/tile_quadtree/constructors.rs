//! Constructors for [`TileQuadtree`].

use super::{BBox, Node, TileQuadtree};
use crate::{GeoBBox, TileBBox, validate_zoom_level};
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

		let Some(bbox) = BBox::from_bbox(bbox) else {
			return TileQuadtree::new_empty(level).expect("level already validated");
		};

		let root = Node::build_node(&BBox::root(level), &bbox);
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
		let root = Node::from_morton_sorted(&codes, &BBox::root(level));
		Ok(TileQuadtree { level, root })
	}

	/// Validate that a TileBBox belongs to the given zoom level.
	pub(crate) fn check_zoom(&self, zoom: u8) -> Result<()> {
		ensure!(
			self.level == zoom,
			"quadtree level {} does not match zoom {}",
			self.level,
			zoom
		);
		Ok(())
	}
}

impl Node {
	/// Recursively build a quadtree node for `cell` against `bbox` (both using
	/// exclusive max coordinates).
	pub fn build_node(cell: &BBox, bbox: &BBox) -> Node {
		let Some(clip) = cell.intersection(bbox) else {
			return Node::Empty;
		};
		if clip.covers(cell) {
			return Node::Full;
		}
		// At size == 1, overlap implies coverage for tile-aligned bboxes,
		// so this branch is defensive — it should never fire in practice.
		if cell.size() == 1 {
			return Node::Full;
		}
		let quads = cell.quadrants();
		Node::new_partial(std::array::from_fn(|i| Node::build_node(&quads[i], bbox)))
	}

	/// Build a `Node` from a sorted, deduplicated slice of Morton codes.
	///
	/// The slice must only contain codes for tiles within `cell` and be sorted
	/// ascending. Each quadrant split uses `partition_point` with a trivial
	/// `u64 <` comparison — no Morton recomputation per tile.
	pub(super) fn from_morton_sorted(codes: &[u64], cell: &BBox) -> Node {
		if codes.is_empty() {
			return Node::Empty;
		}
		let size = cell.size();
		if codes.len() as u64 == size * size {
			return Node::Full;
		}
		if size == 1 {
			return Node::Full;
		}
		let quads = cell.quadrants();
		let (x_off, y_off) = (cell.x_min, cell.y_min);
		let mid_x = quads[0].x_max;
		let mid_y = quads[0].y_max;
		// Morton code of the first tile in each quadrant.
		let ne_base = morton(mid_x, y_off);
		let sw_base = morton(x_off, mid_y);
		let se_base = morton(mid_x, mid_y);

		let ne_start = codes.partition_point(|&c| c < ne_base);
		let sw_start = codes.partition_point(|&c| c < sw_base);
		let se_start = codes.partition_point(|&c| c < se_base);

		Node::new_partial([
			Node::from_morton_sorted(&codes[..ne_start], &quads[0]),
			Node::from_morton_sorted(&codes[ne_start..sw_start], &quads[1]),
			Node::from_morton_sorted(&codes[sw_start..se_start], &quads[2]),
			Node::from_morton_sorted(&codes[se_start..], &quads[3]),
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileCoord;
	use anyhow::Result;

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn new_empty_and_full() {
		let e = TileQuadtree::new_empty(4).unwrap();
		assert!(e.is_empty());
		assert!(!e.is_full());
		assert_eq!(e.level(), 4);
		assert_eq!(e.count_tiles(), 0);

		let f = TileQuadtree::new_full(3).unwrap();
		assert!(!f.is_empty());
		assert!(f.is_full());
		assert_eq!(f.level(), 3);
		assert_eq!(f.count_tiles(), 64); // 8×8
	}

	#[test]
	fn from_bbox_empty() -> Result<()> {
		let b = TileBBox::new_empty(5)?;
		let t = TileQuadtree::from_bbox(&b);
		assert!(t.is_empty());
		Ok(())
	}

	#[test]
	fn from_bbox_full() -> Result<()> {
		let b = TileBBox::new_full(3)?;
		let t = TileQuadtree::from_bbox(&b);
		assert!(t.is_full());
		assert_eq!(t.count_tiles(), 64);
		Ok(())
	}

	#[test]
	fn from_bbox_partial() -> Result<()> {
		// z=2: 4×4 grid, bbox covers x=0..1, y=0..1 (2×2 = 4 tiles)
		let b = bbox(2, 0, 0, 1, 1);
		let t = TileQuadtree::from_bbox(&b);
		assert!(!t.is_empty());
		assert!(!t.is_full());
		assert_eq!(t.count_tiles(), 4);
		Ok(())
	}

	#[test]
	fn from_geo_bbox() -> Result<()> {
		let geo = GeoBBox::new(8.0, 51.0, 8.5, 51.5).unwrap();
		let t = TileQuadtree::from_geo_bbox(9, &geo)?;
		assert!(!t.is_empty());
		Ok(())
	}

	#[test]
	fn from_tile_iter_empty() -> Result<()> {
		let t = TileQuadtree::from_tile_coords(4, &[])?;
		assert!(t.is_empty());
		assert_eq!(t.count_tiles(), 0);
		Ok(())
	}

	#[test]
	fn from_tile_iter_full() -> Result<()> {
		// Provide all 4×4 = 16 tiles at zoom 2.
		let all: Vec<(u32, u32)> = (0u32..4).flat_map(|x| (0u32..4).map(move |y| (x, y))).collect();
		let t = TileQuadtree::from_tile_coords(2, &all)?;
		assert!(t.is_full());
		assert_eq!(t.count_tiles(), 16);
		Ok(())
	}

	#[test]
	fn from_tile_iter_single_tile() -> Result<()> {
		let t = TileQuadtree::from_tile_coords(3, &[(5, 3)])?;
		assert!(!t.is_empty());
		assert!(!t.is_full());
		assert_eq!(t.count_tiles(), 1);
		assert!(t.includes_coord(&coord(3, 5, 3)));
		assert!(!t.includes_coord(&coord(3, 4, 3)));
		Ok(())
	}

	#[test]
	fn from_tile_iter_matches_sequential_insert() -> Result<()> {
		// Build the same tree two ways and assert they are equal.
		let tiles = vec![(1u32, 2u32), (3, 4), (5, 6), (0, 7), (7, 0)];

		// Sequential insertion
		let mut seq = TileQuadtree::new_empty(3).unwrap();
		for &(x, y) in &tiles {
			seq.insert_coord(&coord(3, x, y))?;
		}

		// Batch construction
		let batch = TileQuadtree::from_tile_coords(3, &tiles)?;

		assert_eq!(seq, batch);
		Ok(())
	}

	#[test]
	fn from_tile_iter_deduplicates() -> Result<()> {
		// Duplicate tiles must not inflate the count.
		let t = TileQuadtree::from_tile_coords(3, &[(2u32, 2u32), (2, 2), (2, 2)])?;
		assert_eq!(t.count_tiles(), 1);
		Ok(())
	}

	#[test]
	fn from_tile_iter_rectangular_block_matches_from_bbox() -> Result<()> {
		// A contiguous rectangle should collapse to the same tree as from_bbox.
		let b = bbox(4, 3, 5, 7, 9);
		let tiles: Vec<(u32, u32)> = (3u32..=7).flat_map(|x| (5u32..=9).map(move |y| (x, y))).collect();
		let batch = TileQuadtree::from_tile_coords(4, &tiles)?;
		let reference = TileQuadtree::from_bbox(&b);
		assert_eq!(batch, reference);
		Ok(())
	}
}
