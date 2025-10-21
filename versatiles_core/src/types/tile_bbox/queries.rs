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

	/// Returns whether the bounding box is empty.
	///
	/// A `TileBBox` is empty if its width or height is zero.
	/// Empty bounding boxes are often used as placeholders or as neutral elements
	/// in merge/intersect operations.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let empty = TileBBox::new_empty(5).unwrap();
	/// assert!(empty.is_empty());
	/// ```
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.width() == 0 || self.height() == 0
	}

	/// Returns the total number of tiles covered by this bbox.
	///
	/// The count is computed as `width × height`.
	/// Returns `0` if the bbox is empty.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let bb = TileBBox::from_min_and_max(4, 5, 6, 7, 9).unwrap();
	/// assert_eq!(bb.count_tiles(), 12);
	/// ```
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		u64::from(self.width()) * u64::from(self.height())
	}

	/// Determines if the bounding box covers the entire range of tiles at its zoom level.
	///
	/// # Returns
	///
	/// * `true` if the bounding box is full.
	/// * `false` otherwise.
	///
	/// # Note
	///
	/// This method is primarily used for testing purposes.
	#[cfg(test)]
	#[must_use]
	pub fn is_full(&self) -> bool {
		let max = self.max_count();
		self.x_min() == 0 && self.y_min() == 0 && self.width() == max && self.height() == max
	}

	/// Returns the maximum tile count along one axis at this zoom level.
	/// Equivalent to `2^level`.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// assert_eq!(TileBBox::new_empty(5).unwrap().max_count(), 32);
	/// ```
	#[must_use]
	pub fn max_count(&self) -> u32 {
		1u32 << self.level
	}

	/// Checks if the bounding box contains a specific tile coordinate (`TileCoord`) at the same zoom level.
	///
	/// # Arguments
	///
	/// * `coord` - Reference to the tile coordinate to check.
	///
	/// # Returns
	///
	/// * `true` if the coordinate is within the bounding box and at the same zoom level.
	/// * `false` otherwise.
	#[must_use]
	pub fn contains(&self, coord: &TileCoord) -> bool {
		coord.level == self.level
			&& coord.x >= self.x_min()
			&& coord.x <= self.x_max()
			&& coord.y >= self.y_min()
			&& coord.y <= self.y_max()
	}

	/// Returns whether this bbox completely contains another bbox at the same level.
	///
	/// Returns an error if the zoom levels differ.
	/// Empty bboxes never contain anything.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let outer = TileBBox::from_min_and_max(5, 10, 10, 20, 20).unwrap();
	/// let inner = TileBBox::from_min_and_max(5, 12, 12, 18, 18).unwrap();
	/// assert!(outer.try_contains_bbox(&inner).unwrap());
	/// ```
	pub fn try_contains_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		ensure!(
			self.level == bbox.level,
			"Cannot compare TileBBox with level={} with TileBBox with level={}",
			bbox.level,
			self.level
		);

		if self.is_empty() || bbox.is_empty() {
			return Ok(false);
		}

		Ok(self.x_min() <= bbox.x_min()
			&& self.x_max() >= bbox.x_max()
			&& self.y_min() <= bbox.y_min()
			&& self.y_max() >= bbox.y_max())
	}

	// -------------------------------------------------------------------------
	// Include and Intersect Operations
	// -------------------------------------------------------------------------

	/// Checks if two bounding boxes overlap in tile space.
	///
	/// Overlap is defined as intersecting or touching ranges in both X and Y.
	/// Returns an error if levels differ.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let a = TileBBox::from_min_and_max(5, 10, 10, 20, 20).unwrap();
	/// let b = TileBBox::from_min_and_max(5, 20, 15, 22, 18).unwrap();
	/// assert!(a.overlaps_bbox(&b).unwrap());
	/// ```
	pub fn overlaps_bbox(&self, bbox: &TileBBox) -> Result<bool> {
		if self.level != bbox.level {
			return Err(anyhow::anyhow!(
				"Cannot compare TileBBox with level={} with TileBBox with level={}",
				bbox.level,
				self.level
			));
		}

		if self.is_empty() || bbox.is_empty() {
			return Ok(false);
		}

		Ok(self.x_min() <= bbox.x_max()
			&& self.x_max() >= bbox.x_min()
			&& self.y_min() <= bbox.y_max()
			&& self.y_max() >= bbox.y_min())
	}

	#[must_use]
	pub fn min_corner(&self) -> TileCoord {
		TileCoord::new(self.level, self.x_min(), self.y_min()).unwrap()
	}

	#[must_use]
	pub fn max_corner(&self) -> TileCoord {
		TileCoord::new(self.level, self.x_max(), self.y_max()).unwrap()
	}

	#[must_use]
	pub fn dimensions(&self) -> (u32, u32) {
		(self.width(), self.height())
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
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let bb = TileBBox::from_min_and_max(4, 8, 12, 11, 15).unwrap();
	/// let q0 = bb.get_quadrant(0).unwrap();
	/// assert_eq!(q0.as_array(), [8, 12, 9, 13]);
	/// ```
	#[context("getting quadrant {quadrant} of TileBBox {self:?}")]
	pub fn get_quadrant(&self, quadrant: u8) -> Result<TileBBox> {
		if self.is_empty() {
			return Ok(*self);
		}

		ensure!(quadrant < 4, "quadrant must be in 0..3");
		ensure!(!self.is_empty(), "cannot get quadrant of an empty TileBBox");
		ensure!(
			self.width().is_multiple_of(2),
			"cannot get quadrant of a TileBBox with odd width"
		);
		ensure!(
			self.height().is_multiple_of(2),
			"cannot get quadrant of a TileBBox with odd height"
		);

		let x = self.x_min();
		let y = self.y_min();
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
	///
	/// # Example
	/// ```
	/// # use versatiles_core::{TileBBox, TileCoord};
	/// let bb = TileBBox::from_min_and_max(4, 5, 6, 7, 7).unwrap();
	/// let coord = TileCoord::new(4, 6, 7).unwrap();
	/// assert_eq!(bb.index_of(&coord).unwrap(), 4);
	/// ```
	pub fn index_of(&self, coord: &TileCoord) -> Result<u64> {
		ensure!(
			self.contains(coord),
			"Coordinate {coord:?} is not within the bounding box {self:?}",
		);

		let x = u64::from(coord.x - self.x_min());
		let y = u64::from(coord.y - self.y_min());
		let index = y * u64::from(self.width()) + x;

		Ok(index)
	}

	/// Returns the tile coordinate at a given linear index.
	///
	/// Inverse of [`index_of`]. The index must be smaller than `count_tiles()`.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::{TileBBox, TileCoord};
	/// let bb = TileBBox::from_min_and_max(4, 5, 6, 7, 7).unwrap();
	/// assert_eq!(bb.coord_at_index(0).unwrap(), TileCoord::new(4, 5, 6).unwrap());
	/// ```
	pub fn coord_at_index(&self, index: u64) -> Result<TileCoord> {
		ensure!(index < self.count_tiles(), "index {index} out of bounds");

		let width = u64::from(self.width());
		let x = index.rem(width) as u32 + self.x_min();
		let y = index.div(width) as u32 + self.y_min();
		TileCoord::new(self.level, x, y)
	}

	/// Returns the maximum valid tile coordinate index at this zoom level.
	/// Equivalent to `2^level - 1`.
	#[must_use]
	pub fn max_coord(&self) -> u32 {
		(1u32 << self.level) - 1
	}

	/// Returns the bbox as an array `[x_min, y_min, x_max, y_max]`.
	/// Useful for serialization or equality checks.
	pub fn as_array(&self) -> [u32; 4] {
		[self.x_min(), self.y_min(), self.x_max(), self.y_max()]
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
		assert!(a.contains(&tc(5, 15, 15)));
		assert!(!a.contains(&tc(6, 15, 15))); // level mismatch
		assert!(!a.contains(&tc(5, 25, 15))); // outside

		// try_contains_bbox
		assert!(a.try_contains_bbox(&inner)?);
		assert!(!a.try_contains_bbox(&edge_touch)?); // inner extends beyond
		assert!(!a.try_contains_bbox(&disjoint)?);
		assert!(a.try_contains_bbox(&TileBBox::new_empty(5)?)? == false);
		assert!(a.try_contains_bbox(&bb(6, 12, 12, 18, 18)).is_err()); // level mismatch

		// overlaps_bbox (inclusive on shared edge)
		assert!(a.overlaps_bbox(&inner)?);
		assert!(a.overlaps_bbox(&edge_touch)?); // edge contact counts as overlap by implementation
		assert!(!a.overlaps_bbox(&disjoint)?);
		assert!(a.overlaps_bbox(&bb(6, 0, 0, 0, 0)).is_err());
		Ok(())
	}

	// ------------------------------ Corners & dimensions ------------------------------
	#[test]
	fn corners_and_dimensions() -> Result<()> {
		let b = bb(4, 5, 6, 7, 9); // 3x4
		assert_eq!(b.min_corner(), tc(4, 5, 6));
		assert_eq!(b.max_corner(), tc(4, 7, 9));
		assert_eq!(b.dimensions(), (3, 4));
		Ok(())
	}

	// ------------------------------ Quadrants ------------------------------
	#[test]
	fn get_quadrant_happy_path() -> Result<()> {
		// 4x4 region divisible by 2
		let b = bb(4, 8, 12, 11, 15);
		let q0 = b.get_quadrant(0)?; // top-left
		let q1 = b.get_quadrant(1)?; // top-right
		let q2 = b.get_quadrant(2)?; // bottom-left
		let q3 = b.get_quadrant(3)?; // bottom-right
		assert_eq!(q0.as_array(), [8, 12, 9, 13]);
		assert_eq!(q1.as_array(), [10, 12, 11, 13]);
		assert_eq!(q2.as_array(), [8, 14, 9, 15]);
		assert_eq!(q3.as_array(), [10, 14, 11, 15]);
		Ok(())
	}

	#[test]
	fn get_quadrant_errors_on_bad_input() -> Result<()> {
		// odd width
		let b = bb(4, 8, 12, 10, 15); // width=3, height=4
		assert!(b.get_quadrant(0).is_err());
		// odd height
		let b = bb(4, 8, 12, 11, 14); // width=4, height=3
		assert!(b.get_quadrant(0).is_err());
		// invalid quadrant index
		let b = bb(4, 8, 12, 11, 15);
		assert!(b.get_quadrant(4).is_err());
		Ok(())
	}

	#[test]
	fn get_quadrant_on_empty_returns_self() -> Result<()> {
		let e = TileBBox::new_empty(5)?;
		let q = e.get_quadrant(0)?;
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
		assert_eq!(b.as_array(), [10, 20, 30, 40]);
		Ok(())
	}
}
