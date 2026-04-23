//! Query and inspection methods for `TileBBox`.
//!
//! This module implements read-only operations on tile-aligned bounding boxes,
//! including containment checks, overlap tests, coordinate indexing, and quadrant
//! subdivision. All methods respect the zoom level and coordinate boundaries of
//! the bounding box.
//!
//! Most methods return `bool` or `Result<T>` depending on whether level mismatches
//! or invalid states (such as odd dimensions) must be handled.

use crate::{TileBBox, TileCoord};
use anyhow::{Result, ensure};
use std::ops::{Div, Rem};
use versatiles_derive::context;

impl TileBBox {
	// -------------------------------------------------------------------------
	// Basic Queries
	// -------------------------------------------------------------------------

	/// Returns the zoom level of this bounding box.
	#[must_use]
	pub fn level(&self) -> u8 {
		self.level
	}

	pub fn min_tile(&self) -> Result<TileCoord> {
		ensure!(!self.is_empty(), "cannot get min tile of an empty TileBBox");
		TileCoord::new(self.level, self.x_min()?, self.y_min()?)
	}

	pub fn max_tile(&self) -> Result<TileCoord> {
		ensure!(!self.is_empty(), "cannot get max tile of an empty TileBBox");
		TileCoord::new(self.level, self.x_max()?, self.y_max()?)
	}

	/// Returns one of the four quadrants of this bbox.
	///
	/// Quadrants are numbered:
	/// * 0 – top-left
	/// * 1 – top-right
	/// * 2 – bottom-left
	/// * 3 – bottom-right
	///
	/// The bbox must have even width and height. Returns an error otherwise.
	#[context("getting quadrant {quadrant} of TileBBox {self:?}")]
	pub fn quadrant(&self, quadrant: u8) -> Result<TileBBox> {
		if self.is_empty() {
			return Ok(*self);
		}

		ensure!(quadrant < 4, "quadrant must be in 0..3");
		ensure!(
			self.width().is_multiple_of(2),
			"cannot get quadrant of a TileBBox with odd width"
		);
		ensure!(
			self.height().is_multiple_of(2),
			"cannot get quadrant of a TileBBox with odd height"
		);

		let x = self.x_min()?;
		let y = self.y_min()?;
		let w = self.width() / 2;
		let h = self.height() / 2;

		let bbox = match quadrant {
			0 => TileBBox::from_min_and_size(self.level, x, y, w, h)?, // Top-left
			1 => TileBBox::from_min_and_size(self.level, x + w, y, w, h)?, // Top-right
			2 => TileBBox::from_min_and_size(self.level, x, y + h, w, h)?, // Bottom-left
			3 => TileBBox::from_min_and_size(self.level, x + w, y + h, w, h)?, // Bottom-right
			_ => unreachable!("quadrant < 4 was checked above"),
		};

		Ok(bbox)
	}

	/// Returns the linear index of a tile coordinate within this bbox.
	///
	/// Indexing is row-major: X increases fastest, then Y.
	/// Returns an error if the coordinate lies outside the bbox.
	pub fn index_of(&self, coord: &TileCoord) -> Result<u64> {
		ensure!(!self.is_empty(), "cannot get index in an empty TileBBox");
		ensure!(self.level == coord.level);
		ensure!(
			self.includes_coord(coord),
			"Coordinate {coord:?} is not within the bounding box {self:?}",
		);

		let x = u64::from(coord.x - self.x_min()?);
		let y = u64::from(coord.y - self.y_min()?);
		let index = y * u64::from(self.width()) + x;

		Ok(index)
	}

	/// Returns the tile coordinate at a given linear index.
	///
	/// Inverse of [`TileBBox::index_of`]. The index must be smaller than `count_tiles()`.
	pub fn coord_at_index(&self, index: u64) -> Result<TileCoord> {
		ensure!(!self.is_empty(), "cannot get coord from an empty TileBBox");
		ensure!(index < self.count_tiles(), "index {index} out of bounds");

		let width = u64::from(self.width());
		let x = u32::try_from(index.rem(width)).expect("index remainder must fit in u32") + self.x_min()?;
		let y = u32::try_from(index.div(width)).expect("index quotient must fit in u32") + self.y_min()?;
		TileCoord::new(self.level, x, y)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use rstest::rstest;

	fn bb(z: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(z, x0, y0, x1, y1).unwrap()
	}
	fn tc(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	// ── Basic queries: is_empty / is_full / count_tiles / width / height ──

	/// (bbox, is_empty, is_full, count_tiles, width, height).
	#[rstest]
	#[case::empty_z5(TileBBox::new_empty(5).unwrap(), true, false, 0, 0, 0)]
	#[case::full_z3(TileBBox::new_full(3).unwrap(), false, true, 64, 8, 8)]
	#[case::partial_3x4(bb(4, 5, 6, 7, 9), false, false, 12, 3, 4)]
	#[case::partial_11x11(bb(6, 5, 10, 15, 20), false, false, 121, 11, 11)]
	fn basic_queries_cases(
		#[case] b: TileBBox,
		#[case] empty: bool,
		#[case] full: bool,
		#[case] count: u64,
		#[case] width: u32,
		#[case] height: u32,
	) {
		assert_eq!(b.is_empty(), empty);
		assert_eq!(b.is_full(), full);
		assert_eq!(b.count_tiles(), count);
		assert_eq!(b.width(), width);
		assert_eq!(b.height(), height);
		assert_eq!(b.dimensions(), (width, height));
	}

	/// At zoom z, `max_count()` is 2^z tiles per axis, `max_coord()` is 2^z − 1.
	#[rstest]
	#[case(0, 1, 0)]
	#[case(1, 2, 1)]
	#[case(3, 8, 7)]
	#[case(5, 32, 31)]
	fn axis_bounds_are_powers_of_two(#[case] level: u8, #[case] max_count: u32, #[case] max_coord: u32) {
		let b = TileBBox::new_empty(level).unwrap();
		assert_eq!(b.max_count(), max_count);
		assert_eq!(b.max_coord(), max_coord);
	}

	// ── Contains / overlaps ─────────────────────────────────────────────────

	/// a = bbox(5, 10,10,20,20) against various other bboxes.
	#[rstest]
	#[case::inner(bb(5, 12, 12, 18, 18), true, true)]
	#[case::edge_touch(bb(5, 20, 12, 22, 18), false, true)]
	#[case::disjoint(bb(5, 30, 30, 31, 31), false, false)]
	#[case::empty(TileBBox::new_empty(5).unwrap(), true, false)]
	fn includes_bbox_and_intersects_bbox(#[case] other: TileBBox, #[case] includes: bool, #[case] intersects: bool) {
		let a = bb(5, 10, 10, 20, 20);
		assert_eq!(a.includes_bbox(&other), includes);
		assert_eq!(a.intersects_bbox(&other), intersects);
	}

	// ── Corners, dimensions, as_array ──────────────────────────────────────

	#[test]
	fn corners_and_as_array() -> Result<()> {
		let b = bb(4, 5, 6, 7, 9); // 3x4
		assert_eq!(b.min_tile()?, tc(4, 5, 6));
		assert_eq!(b.max_tile()?, tc(4, 7, 9));
		assert_eq!(b.to_array()?, [5, 6, 7, 9]);
		Ok(())
	}

	#[test]
	fn to_string_is_level_and_bounds() {
		assert_eq!(bb(5, 1, 2, 3, 4).to_string(), "5:[1,2,3,4]");
	}

	// ── Quadrants: happy paths for all four quadrant indices ──────────────

	#[rstest]
	#[case::top_left(0, [8, 12, 9, 13])]
	#[case::top_right(1, [10, 12, 11, 13])]
	#[case::bottom_left(2, [8, 14, 9, 15])]
	#[case::bottom_right(3, [10, 14, 11, 15])]
	fn quadrant_happy_path(#[case] idx: u8, #[case] expected: [u32; 4]) -> Result<()> {
		let b = bb(4, 8, 12, 11, 15); // 4x4, aligned
		assert_eq!(b.quadrant(idx)?.to_array()?, expected);
		Ok(())
	}

	#[rstest]
	#[case::odd_width(bb(4, 8, 12, 10, 15), 0, true)] // width=3
	#[case::odd_height(bb(4, 8, 12, 11, 14), 0, true)] // height=3
	#[case::invalid_index(bb(4, 8, 12, 11, 15), 4, true)] // OOB quadrant
	#[case::empty_returns_self(TileBBox::new_empty(5).unwrap(), 0, false)]
	fn quadrant_error_and_empty_cases(#[case] b: TileBBox, #[case] q: u8, #[case] expect_err: bool) {
		let r = b.quadrant(q);
		assert_eq!(r.is_err(), expect_err);
		if !expect_err {
			// Empty passes through unchanged.
			assert_eq!(r.unwrap(), b);
		}
	}

	// ── Index mapping ───────────────────────────────────────────────────────

	/// `index_of(coord)` on bbox(8, 100,100,199,199) — various coords.
	#[rstest]
	#[case::top_left((8, 100, 100), 0)]
	#[case::next_x((8, 101, 100), 1)]
	#[case::right_edge((8, 199, 100), 99)]
	#[case::next_y((8, 100, 101), 100)]
	#[case::bottom_left((8, 100, 199), 9900)]
	#[case::bottom_right((8, 199, 199), 9999)]
	fn index_of_cases(#[case] coord: (u8, u32, u32), #[case] expected: u64) -> Result<()> {
		let b = bb(8, 100, 100, 199, 199);
		let (l, x, y) = coord;
		assert_eq!(b.index_of(&tc(l, x, y))?, expected);
		Ok(())
	}

	/// `coord_at_index` on bbox(4, 5,10,7,12) — roundtrip matches `index_of`.
	#[rstest]
	#[case(0, (4, 5, 10))]
	#[case(1, (4, 6, 10))]
	#[case(2, (4, 7, 10))]
	#[case(3, (4, 5, 11))]
	#[case(8, (4, 7, 12))]
	fn coord_at_index_cases(#[case] index: u64, #[case] expected: (u8, u32, u32)) -> Result<()> {
		let b = bb(4, 5, 10, 7, 12);
		let (l, x, y) = expected;
		let c = tc(l, x, y);
		assert_eq!(b.coord_at_index(index)?, c);
		// Roundtrip must match.
		assert_eq!(b.index_of(&c)?, index);
		Ok(())
	}

	/// Error cases for the index/coord mapping.
	#[rstest]
	#[case::outside_bbox(bb(4, 5, 10, 7, 12), Some(tc(4, 4, 9)), None)]
	#[case::wrong_zoom(bb(4, 5, 10, 7, 12), Some(tc(5, 5, 10)), None)]
	#[case::index_oob(bb(4, 5, 10, 7, 12), None, Some(9))]
	fn index_mapping_error_cases(
		#[case] b: TileBBox,
		#[case] bad_coord: Option<TileCoord>,
		#[case] bad_index: Option<u64>,
	) {
		if let Some(c) = bad_coord {
			assert!(b.index_of(&c).is_err());
		}
		if let Some(i) = bad_index {
			assert!(b.coord_at_index(i).is_err());
		}
	}
}
