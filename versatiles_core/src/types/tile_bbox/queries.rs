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
			_ => unreachable!(),
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

	// ------------------------------ Basic queries ------------------------------
	#[test]
	fn is_empty_and_count_tiles() -> Result<()> {
		let e = TileBBox::new_empty(5)?;
		assert!(e.is_empty());
		assert_eq!(e.count_tiles(), 0);

		let b = bb(4, 5, 6, 7, 9); // 3x4
		assert!(!b.is_empty());
		assert_eq!(b.count_tiles(), 12);
		Ok(())
	}

	#[rstest]
	#[case(0, 1)]
	#[case(1, 2)]
	#[case(5, 32)]
	fn max_count_matches_power_of_two(#[case] level: u8, #[case] expect: u32) -> Result<()> {
		let b = TileBBox::new_empty(level)?;
		assert_eq!(b.max_count(), expect);
		Ok(())
	}

	#[test]
	fn is_full_works_in_tests_only() -> Result<()> {
		let f = TileBBox::new_full(3)?;
		assert!(f.is_full());
		let p = bb(3, 0, 0, 6, 7); // not full
		assert!(!p.is_full());
		Ok(())
	}

	// ------------------------------ Contains / overlaps / try_contains_bbox ------------------------------
	#[test]
	fn contains_and_overlaps_and_try_contains() -> Result<()> {
		let a = bb(5, 10, 10, 20, 20);
		let inner = bb(5, 12, 12, 18, 18);
		let edge_touch = bb(5, 20, 12, 22, 18); // touches at x=20
		let disjoint = bb(5, 30, 30, 31, 31);

		// contains (TileCoord)
		assert!(a.includes_coord(&tc(5, 15, 15)));
		assert!(!a.includes_coord(&tc(5, 25, 15))); // outside

		assert!(a.includes_bbox(&inner));
		assert!(!a.includes_bbox(&edge_touch)); // inner extends beyond
		assert!(!a.includes_bbox(&disjoint));
		assert!(a.includes_bbox(&TileBBox::new_empty(5).unwrap())); // empty set is always a subset

		// overlaps_bbox (inclusive on shared edge)
		assert!(a.intersects_bbox(&inner));
		assert!(a.intersects_bbox(&edge_touch)); // edge contact counts as overlap by implementation
		assert!(!a.intersects_bbox(&disjoint));
		Ok(())
	}

	// ------------------------------ Corners & dimensions ------------------------------
	#[test]
	fn corners_and_dimensions() -> Result<()> {
		let b = bb(4, 5, 6, 7, 9); // 3x4
		assert_eq!(b.min_tile()?, tc(4, 5, 6));
		assert_eq!(b.max_tile()?, tc(4, 7, 9));
		assert_eq!(b.dimensions(), (3, 4));
		Ok(())
	}

	// ------------------------------ Quadrants ------------------------------
	#[test]
	fn get_quadrant_happy_path() -> Result<()> {
		// 4x4 region divisible by 2
		let b = bb(4, 8, 12, 11, 15);
		let q0 = b.quadrant(0)?; // top-left
		let q1 = b.quadrant(1)?; // top-right
		let q2 = b.quadrant(2)?; // bottom-left
		let q3 = b.quadrant(3)?; // bottom-right
		assert_eq!(q0.to_array()?, [8, 12, 9, 13]);
		assert_eq!(q1.to_array()?, [10, 12, 11, 13]);
		assert_eq!(q2.to_array()?, [8, 14, 9, 15]);
		assert_eq!(q3.to_array()?, [10, 14, 11, 15]);
		Ok(())
	}

	#[test]
	fn get_quadrant_errors_on_bad_input() -> Result<()> {
		// odd width
		let b = bb(4, 8, 12, 10, 15); // width=3, height=4
		assert!(b.quadrant(0).is_err());
		// odd height
		let b = bb(4, 8, 12, 11, 14); // width=4, height=3
		assert!(b.quadrant(0).is_err());
		// invalid quadrant index
		let b = bb(4, 8, 12, 11, 15);
		assert!(b.quadrant(4).is_err());
		Ok(())
	}

	#[test]
	fn get_quadrant_on_empty_returns_self() -> Result<()> {
		let e = TileBBox::new_empty(5)?;
		let q = e.quadrant(0)?;
		assert_eq!(q, e);
		Ok(())
	}

	// ------------------------------ Index mapping ------------------------------
	#[test]
	fn index_of_and_coord_at_index_roundtrip() -> Result<()> {
		let b = bb(4, 5, 6, 7, 7); // 3x2

		// index_of
		assert_eq!(b.index_of(&tc(4, 5, 6))?, 0);
		assert_eq!(b.index_of(&tc(4, 7, 6))?, 2);
		assert_eq!(b.index_of(&tc(4, 7, 7))?, 5);

		// coord_at_index
		assert_eq!(b.coord_at_index(0)?, tc(4, 5, 6));
		assert_eq!(b.coord_at_index(2)?, tc(4, 7, 6));
		assert_eq!(b.coord_at_index(5)?, tc(4, 7, 7));

		// Errors
		assert!(b.index_of(&tc(4, 9, 9)).is_err()); // outside
		assert!(b.coord_at_index(6).is_err()); // OOB
		Ok(())
	}

	// ------------------------------ max_coord / as_array ------------------------------
	#[rstest]
	#[case(0, 0)]
	#[case(1, 1)]
	#[case(3, 7)]
	fn max_coord_is_2_pow_z_minus_1(#[case] level: u8, #[case] expect: u32) -> Result<()> {
		let b = TileBBox::new_empty(level)?;
		assert_eq!(b.max_coord(), expect);
		Ok(())
	}

	#[test]
	fn as_array_matches_minmax() -> Result<()> {
		let b = bb(6, 10, 20, 30, 40);
		assert_eq!(b.to_array()?, [10, 20, 30, 40]);
		Ok(())
	}

	#[test]
	fn test_is_full() -> Result<()> {
		let bbox = TileBBox::new_full(4)?;
		assert!(bbox.is_full(), "Expected bbox ({bbox:?}) to be full");
		Ok(())
	}

	#[test]
	fn test_is_empty() -> Result<()> {
		let empty_bbox = TileBBox::new_empty(4)?;
		assert!(empty_bbox.is_empty());

		let non_empty_bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
		assert!(!non_empty_bbox.is_empty());

		Ok(())
	}

	#[test]
	fn test_width_height() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
		assert_eq!(bbox.width(), 11);
		assert_eq!(bbox.height(), 11);

		let empty_bbox = TileBBox::new_empty(4)?;
		assert_eq!(empty_bbox.width(), 0);
		assert_eq!(empty_bbox.height(), 0);

		Ok(())
	}

	#[test]
	fn test_count_tiles() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
		assert_eq!(bbox.count_tiles(), 121);

		let empty_bbox = TileBBox::new_empty(4)?;
		assert_eq!(empty_bbox.count_tiles(), 0);

		Ok(())
	}

	#[rstest]
	#[case((8, 100, 100, 199, 199), (8, 100, 100), 0)]
	#[case((8, 100, 100, 199, 199), (8, 101, 100), 1)]
	#[case((8, 100, 100, 199, 199), (8, 199, 100), 99)]
	#[case((8, 100, 100, 199, 199), (8, 100, 101), 100)]
	#[case((8, 100, 100, 199, 199), (8, 100, 199), 9900)]
	#[case((8, 100, 100, 199, 199), (8, 199, 199), 9999)]
	fn tile_index_cases(
		#[case] bbox: (u8, u32, u32, u32, u32),
		#[case] coord: (u8, u32, u32),
		#[case] expected: u64,
	) -> Result<()> {
		let (l, x0, y0, x1, y1) = bbox;
		let bbox = TileBBox::from_min_and_max(l, x0, y0, x1, y1)?;
		let (cl, cx, cy) = coord;
		let tc = tc(cl, cx, cy);
		assert_eq!(bbox.index_of(&tc)?, expected);
		Ok(())
	}

	#[test]
	fn should_get_correct_tile_index() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;

		assert_eq!(bbox.index_of(&tc(4, 5, 10))?, 0);
		assert_eq!(bbox.index_of(&tc(4, 6, 10))?, 1);
		assert_eq!(bbox.index_of(&tc(4, 7, 10))?, 2);
		assert_eq!(bbox.index_of(&tc(4, 5, 11))?, 3);
		assert_eq!(bbox.index_of(&tc(4, 7, 12))?, 8);

		// Attempt to get index of a coordinate outside the bounding box
		let coord_outside = tc(4, 4, 9);
		let result = bbox.index_of(&coord_outside);
		assert!(result.is_err());

		// Attempt to get index with mismatched zoom level
		let coord_diff_level = tc(5, 5, 10);
		let result = bbox.index_of(&coord_diff_level);
		assert!(result.is_err());

		Ok(())
	}

	#[rstest]
	#[case(0, (4, 5, 10))]
	#[case(1, (4, 6, 10))]
	#[case(2, (4, 7, 10))]
	#[case(3, (4, 5, 11))]
	#[case(8, (4, 7, 12))]
	fn get_coord_by_index_cases(#[case] index: u64, #[case] coord: (u8, u32, u32)) -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;
		let (l, x, y) = coord;
		assert_eq!(bbox.coord_at_index(index)?, tc(l, x, y));
		Ok(())
	}

	#[test]
	fn get_coord_by_index_out_of_bounds() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 7, 12)?;
		assert!(bbox.coord_at_index(9).is_err());
		Ok(())
	}

	#[test]
	fn get_quadrant_errors() -> Result<()> {
		// Empty bbox → Ok(empty)
		let empty = TileBBox::new_empty(4)?;
		assert!(empty.quadrant(0)?.is_empty());
		// Odd width/height → error
		let odd_w = TileBBox::from_min_and_max(4, 0, 0, 2, 3)?; // width=3
		assert!(odd_w.quadrant(0).is_err());
		let odd_h = TileBBox::from_min_and_max(4, 0, 0, 3, 2)?; // height=3
		assert!(odd_h.quadrant(0).is_err());
		// Invalid quadrant index
		let even = TileBBox::from_min_and_max(4, 0, 0, 3, 3)?;
		assert!(even.quadrant(4).is_err());
		Ok(())
	}

	#[test]
	fn max_value_and_string() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(5, 1, 2, 3, 4)?;
		assert_eq!(bbox.max_coord(), (1u32 << 5) - 1);
		assert_eq!(bbox.to_string(), "5:[1,2,3,4]");
		Ok(())
	}
}
