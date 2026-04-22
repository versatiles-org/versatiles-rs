//! Mutating operations for `TileBBox`.
//!
//! This module implements in-place transformations on tile bounding boxes:
//! including tiles or other bboxes, expansion, intersection, shifting,
//! scaling, zoom-level changes, rounding to block boundaries, and vertical
//! flipping to match XYZ/TMS conventions.
//!
//! All operations preserve invariants:
//! * Coordinates are clamped to the valid range for the bbox level.
//! * Empty bboxes remain empty unless explicitly expanded.
//! * Methods that cannot fail are infallible; those that validate inputs
//!   return `anyhow::Result<()>`.

use crate::{MAX_ZOOM_LEVEL, TileBBox, TileCoord, validate_zoom_level};
use anyhow::{Result, ensure};
use std::ops::Div;
use versatiles_derive::context;

impl TileBBox {
	/// Insert a specific tile coordinate `(x, y)` into this bbox.
	///
	/// If the bbox is empty, it becomes the single-tile bbox at `(x, y)`.
	/// Otherwise, the bbox is expanded minimally to include the coordinate.
	///
	/// # Panics
	/// Panics if `x` or `y` are out of range for the current level.
	pub fn insert_xy(&mut self, x: u32, y: u32) {
		assert!(x < self.max_count(), "x ({x}) must be < max ({})", self.max_count());
		assert!(y < self.max_count(), "y ({y}) must be < max ({})", self.max_count());
		if self.is_empty() {
			// Initialize bounding box to the provided coordinate
			self
				.set_min_and_size(x, y, 1, 1)
				.expect("x, y within level bounds");
		} else {
			// Expand bounding box to include the new coordinate
			if x < self.x_min().expect("bbox is non-empty") {
				self.set_x_min(x).expect("x within level bounds");
			} else if x > self.x_max().expect("bbox is non-empty") {
				self.set_x_max(x).expect("x within level bounds");
			}
			if y < self.y_min().expect("bbox is non-empty") {
				self.set_y_min(y).expect("y within level bounds");
			} else if y > self.y_max().expect("bbox is non-empty") {
				self.set_y_max(y).expect("y within level bounds");
			}
		}
	}

	/// Insert a tile coordinate (`TileCoord`) into this bounding box.
	///
	/// Expands the bounding box to encompass the given coordinate. The zoom level of the coordinate
	/// must match the bounding box's zoom level.
	///
	/// # Arguments
	///
	/// * `coord` - Reference to the tile coordinate to insert.
	///
	/// # Returns
	///
	/// * `Ok(())` if insertion is successful.
	/// * `Err(anyhow::Error)` if the zoom levels do not match or other validations fail.
	#[context("Failed to insert TileCoord {coord:?} into TileBBox {self:?}")]
	pub fn insert_coord(&mut self, coord: &TileCoord) -> Result<()> {
		ensure!(
			coord.level == self.level,
			"Cannot insert TileCoord with z={} into TileBBox at z={}",
			coord.level,
			self.level
		);
		self.insert_xy(coord.x, coord.y);
		Ok(())
	}

	/// Adds a buffer to the bbox by expanding its min/max.
	///
	/// Subtracts `size` from the current minimum and adds `size` to the current maximum.
	/// The expansion is **clamped** to the level’s bounds.
	///
	/// This method is infallible and a no-op for empty bboxes.
	pub fn buffer(&mut self, size: u32) {
		if !self.is_empty() {
			let max = self.max_count() - 1;
			self
				.set_min_and_max(
					self.x_min().expect("bbox is non-empty").saturating_sub(size),
					self.y_min().expect("bbox is non-empty").saturating_sub(size),
					self.x_max().expect("bbox is non-empty").saturating_add(size).min(max),
					self.y_max().expect("bbox is non-empty").saturating_add(size).min(max),
				)
				.expect("clamped to level bounds");
		}
	}

	/// Expands the bounding box to include another bounding box.
	///
	/// Merges the extents of `bbox` into this bounding box. Both bounding boxes must be at the same zoom level.
	///
	/// # Arguments
	///
	/// * `bbox` - Reference to the `TileBBox` to insert.
	///
	/// # Returns
	///
	/// * `Ok(())` if insertion is successful.
	/// * `Err(anyhow::Error)` if the zoom levels do not match or other validations fail.
	#[context("Failed to insert TileBBox {bbox:?} into TileBBox {self:?}")]
	pub fn insert_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		ensure!(
			self.level == bbox.level,
			"Cannot include TileBBox with level={} into TileBBox with level={}",
			bbox.level,
			self.level
		);

		if bbox.is_empty() {
			return Ok(()); // Nothing to include
		}

		if self.is_empty() {
			// If current bounding box is empty, adopt the other bounding box
			*self = *bbox;
		} else {
			// Expand bounding box to include the other bounding box
			self.set_min_and_max(
				self.x_min().expect("bbox is non-empty").min(bbox.x_min().expect("bbox is non-empty")),
				self.y_min().expect("bbox is non-empty").min(bbox.y_min().expect("bbox is non-empty")),
				self.x_max().expect("bbox is non-empty").max(bbox.x_max().expect("bbox is non-empty")),
				self.y_max().expect("bbox is non-empty").max(bbox.y_max().expect("bbox is non-empty")),
			)?;
		}

		Ok(())
	}

	/// Shift the bbox by integer offsets `(dx, dy)`.
	///
	/// Negative shifts are **clamped** at zero; the bbox never moves outside the
	/// valid range for its level.
	#[context("Failed to shift TileBBox {self:?} by ({x}, {y})")]
	pub fn shift_by(&mut self, x: i64, y: i64) -> Result<()> {
		if self.is_empty() {
			return Ok(()); // No-op for empty bboxes
		}
		let max = i64::from(self.max_count() - 1);
		let x_min = u32::try_from((i64::from(self.x_min()?) + x).clamp(0, max)).expect("clamped value must fit in u32");
		let y_min = u32::try_from((i64::from(self.y_min()?) + y).clamp(0, max)).expect("clamped value must fit in u32");
		self.set_min_and_size(
			x_min,
			y_min,
			self.width().min(self.max_count() - x_min),
			self.height().min(self.max_count() - y_min),
		)
	}

	/// Move the bbox so its top-left corner is at `(x_min, y_min)`, keeping width and height.
	///
	/// The shift is **clamped** so the bbox stays within the level's valid range.
	/// No-op for empty bboxes.
	#[context("Failed to shift TileBBox {self:?} to ({x_min}, {y_min})")]
	pub fn shift_to(&mut self, x_min: u32, y_min: u32) -> Result<()> {
		if self.is_empty() {
			return Ok(()); // No-op for empty bboxes
		}
		self.set_min_and_size(x_min, y_min, self.width(), self.height())
	}

	/// Scales down the bounding box by a specified factor.
	///
	/// Divides all coordinates by `scale`, effectively reducing the resolution.
	///
	/// # Arguments
	///
	/// * `scale` - Factor by which to scale down the bounding box.
	///
	/// # Panics
	///
	/// Panics if `scale` is zero.
	pub fn scale_down(&mut self, scale: u32) {
		if self.is_empty() {
			return; // No-op for empty bboxes
		}
		assert!(scale > 0, "scale must be greater than 0");
		assert!(scale.is_power_of_two(), "scale must be a power of two");

		self
			.set_min_and_max(
				self.x_min().expect("bbox is non-empty") / scale,
				self.y_min().expect("bbox is non-empty") / scale,
				self.x_max().expect("bbox is non-empty") / scale,
				self.y_max().expect("bbox is non-empty") / scale,
			)
			.expect("scaled bounds remain valid");
	}

	/// Return a downscaled **copy** of this bbox by an integer power-of-two factor.
	#[must_use]
	pub fn scaled_down(&self, scale: u32) -> TileBBox {
		if self.is_empty() {
			return TileBBox::new_empty(self.level).expect("level already validated");
		}
		let mut bbox = *self;
		bbox.scale_down(scale);
		bbox
	}

	/// Scale coordinates by an integer factor (≥ 1).
	///
	/// Expands `(x_max, y_max)` to keep the same **inclusive** extent semantics.
	#[context("Failed to scale up TileBBox {self:?} by factor {scale}")]
	pub fn scale_up(&mut self, scale: u32) -> Result<()> {
		if self.is_empty() {
			return Ok(()); // No-op for empty bboxes
		}
		ensure!(scale > 0, "scale must be greater than 0");

		self.set_min_and_max(
			self.x_min()? * scale,
			self.y_min()? * scale,
			(self.x_max()? + 1) * scale - 1,
			(self.y_max()? + 1) * scale - 1,
		)
	}

	/// Return an upscaled **copy** of this bbox by an integer factor (≥ 1).
	#[context("Failed to scale up TileBBox {self:?} by factor {scale}")]
	pub fn scaled_up(&self, scale: u32) -> Result<TileBBox> {
		let mut bbox = *self;
		bbox.scale_up(scale)?;
		Ok(bbox)
	}

	/// Increase the zoom level by one and multiply coordinates by 2.
	pub fn level_up(&mut self) {
		assert!(self.level < MAX_ZOOM_LEVEL, "level must be less than {MAX_ZOOM_LEVEL}");
		self.level += 1;
		self.scale_up(2).expect("scale up by 2 at valid level");
	}

	/// Decrease the zoom level by one and divide coordinates by 2.
	pub fn level_down(&mut self) {
		assert!(self.level > 0, "level must be greater than 0");
		self.level -= 1;
		self.scale_down(2);
	}

	/// Return a copy of this bbox at the next zoom level (×2 coordinates).
	#[must_use]
	pub fn leveled_up(&self) -> TileBBox {
		let mut c = *self;
		c.level_up();
		c
	}

	/// Return a copy of this bbox at the previous zoom level (÷2 coordinates).
	#[must_use]
	pub fn leveled_down(&self) -> TileBBox {
		let mut c = *self;
		c.level_down();
		c
	}

	/// Convert this bbox to another zoom level, scaling coordinates appropriately.
	#[must_use]
	pub fn at_level(&self, level: u8) -> TileBBox {
		validate_zoom_level(level).expect("level must be <= MAX_ZOOM_LEVEL");

		let mut bbox = *self;
		if level > self.level {
			let scale = 2u32.pow(u32::from(level - self.level));
			bbox.level = level;
			bbox.scale_up(scale).expect("scale up to higher level");
		} else {
			let scale = 2u32.pow(u32::from(self.level - level));
			bbox.scale_down(scale);
			bbox.level = level;
		}
		bbox
	}

	/// Expand the bbox to align its edges to multiples of `block_size` (inclusive max).
	///
	/// The result is clamped to the level's valid coordinate range, so rounding
	/// with a `block_size` larger than `max_count` will not exceed the level bounds.
	pub fn round(&mut self, block_size: u32) {
		if self.is_empty() || block_size <= 1 {
			return; // No-op for empty bboxes
		}
		let max_coord = self.max_count() - 1;
		self
			.set_min_and_max(
				(self.x_min().expect("bbox is non-empty").div(block_size)) * block_size,
				(self.y_min().expect("bbox is non-empty").div(block_size)) * block_size,
				((self.x_max().expect("bbox is non-empty") + 1).div_ceil(block_size) * block_size - 1).min(max_coord),
				((self.y_max().expect("bbox is non-empty") + 1).div_ceil(block_size) * block_size - 1).min(max_coord),
			)
			.expect("clamped to level bounds");
	}

	/// Return a copy rounded to `block_size` boundaries.
	#[must_use]
	pub fn rounded(&self, block_size: u32) -> TileBBox {
		let mut bbox = *self;
		bbox.round(block_size);
		bbox
	}

	/// Flip the bbox vertically (Y axis) within the current level’s range.
	pub fn flip_y(&mut self) {
		if !self.is_empty() {
			self
				.shift_to(
					self.x_min().expect("bbox is non-empty"),
					self.max_coord() - self.y_max().expect("bbox is non-empty"),
				)
				.expect("shift within level bounds");
		}
	}
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
mod tests {
	use super::*;
	use anyhow::Result;
	use rstest::rstest;

	// Helpers
	fn bb(z: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(z, x0, y0, x1, y1).unwrap()
	}

	// ------------------------------ include / include_coord ------------------------------
	#[test]
	fn include_initializes_when_empty_and_expands() -> Result<()> {
		let mut b = TileBBox::new_empty(4)?;
		assert!(b.is_empty());
		b.insert_xy(3, 5);
		assert_eq!(b.to_array()?, [3, 5, 3, 5]);

		b.insert_xy(6, 2); // expand both axes
		assert_eq!(b.to_array()?, [3, 2, 6, 5]);
		Ok(())
	}

	#[test]
	#[should_panic(expected = "x (16) must be < max (16)")]
	fn insert_xy_panics_on_out_of_bounds_x() {
		let mut b = TileBBox::new_empty(4).unwrap(); // max_count=16
		b.insert_xy(16, 0);
	}

	#[test]
	fn insert_coord_level_mismatch_errors() -> Result<()> {
		let mut b = bb(5, 10, 10, 12, 12);
		let tc = TileCoord::new(6, 11, 11)?; // different level
		assert!(b.insert_coord(&tc).is_err());
		Ok(())
	}

	// ------------------------------ expand_by ------------------------------
	#[rstest]
	#[case(bb(3, 1, 1, 2, 2), 1, [0,0,3,3])] // expand in all directions, clamp at bounds
	#[case(bb(3, 6, 6, 7, 7), 5, [1,1,7,7])] // subtract saturates at 0
	#[case(bb(3, 2, 2, 3, 3), 0, [2,2,3,3])] // no-op
	fn expand_by_behaviour(#[case] mut b: TileBBox, #[case] size: u32, #[case] expected: [u32; 4]) {
		b.buffer(size);
		assert_eq!(b.to_array().unwrap(), expected);
	}

	#[test]
	fn expand_by_noop_on_empty() {
		let mut b = TileBBox::new_empty(5).unwrap();
		b.buffer(10);
		assert!(b.is_empty());
	}

	// ------------------------------ insert_bbox ------------------------------
	#[test]
	fn insert_bbox_merges_ranges_and_ignores_empty() -> Result<()> {
		let mut a = bb(4, 4, 4, 6, 6);
		let b = bb(4, 2, 5, 8, 7);
		a.insert_bbox(&b)?;
		assert_eq!(a.to_array()?, [2, 4, 8, 7]);
		let c = TileBBox::new_empty(4)?;
		a.insert_bbox(&c)?; // no change
		assert_eq!(a.to_array()?, [2, 4, 8, 7]);
		Ok(())
	}

	#[test]
	fn insert_bbox_level_mismatch_errors() -> Result<()> {
		let mut a = bb(3, 0, 0, 1, 1);
		let b = bb(4, 0, 0, 1, 1);
		assert!(a.insert_bbox(&b).is_err());
		Ok(())
	}

	// ------------------------------ shift_by / shift_to ------------------------------
	#[test]
	fn shift_by_and_to() -> Result<()> {
		let mut b = bb(8, 5, 6, 7, 8); // 3x3
		b.shift_by(-5, -5)?; // clamp to 0
		assert_eq!(b.to_array()?, [0, 1, 2, 3]);
		b.shift_to(13, 14)?; // move within bounds
		assert_eq!(b.to_array()?, [13, 14, 15, 16]);
		Ok(())
	}

	// ------------------------------ scale down / up (and copies) ------------------------------
	#[test]
	fn scale_down_by_powers_of_two() -> Result<()> {
		let mut b = bb(5, 8, 10, 15, 17); // 8x8 region
		b.scale_down(2);
		assert_eq!(b.to_array()?, [4, 5, 7, 8]);
		b.scale_down(2);
		assert_eq!(b.to_array()?, [2, 2, 3, 4]);
		Ok(())
	}

	#[test]
	#[should_panic(expected = "scale must be a power of two")]
	fn scale_down_non_power_of_two_panics() {
		let mut b = bb(4, 4, 4, 7, 7);
		b.scale_down(3);
	}

	#[test]
	fn scaled_down_is_pure() -> Result<()> {
		let b = bb(5, 8, 10, 15, 17);
		let c = b.scaled_down(4);
		assert_eq!(c.to_array()?, [2, 2, 3, 4]);
		// Original unchanged
		assert_eq!(b, bb(5, 8, 10, 15, 17));
		Ok(())
	}

	#[test]
	fn scale_up_and_scaled_up() -> Result<()> {
		let mut b = bb(6, 2, 3, 4, 5); // 3x3
		b.scale_up(2)?;
		assert_eq!(b.to_array()?, [4, 6, 9, 11]);
		let c = b.scaled_up(2)?;
		assert_eq!(c.to_array()?, [8, 12, 19, 23]);
		Ok(())
	}

	#[test]
	fn scale_up_rejects_zero() {
		let mut b = bb(3, 1, 1, 2, 2);
		assert!(b.scale_up(0).is_err());
	}

	// ------------------------------ level up / down and pure variants ------------------------------
	#[test]
	fn level_up_down_and_pure_variants() -> Result<()> {
		let a = bb(2, 1, 1, 2, 2); // 2x2
		let mut b = a;
		b.level_up(); // z=3, ×2
		assert_eq!(b.level, 3);
		assert_eq!(b.to_array()?, [2, 2, 5, 5]);
		b.level_down(); // back to z=2
		assert_eq!(b.level, 2);
		assert_eq!(b.to_array()?, [1, 1, 2, 2]);

		let up = a.leveled_up();
		assert_eq!(up.level, 3);
		let down = up.leveled_down();
		assert_eq!(down, a);
		Ok(())
	}

	// ------------------------------ at_level ------------------------------
	#[rstest]
	#[case(3)]
	#[case(5)]
	#[case(8)]
	#[case(11)]
	fn at_level_matches_manual_scaling(#[case] target: u8) -> Result<()> {
		let a = bb(8, 3, 4, 6, 7); // 4x4
		let b = a.at_level(target);
		if target >= 8 {
			let scale = 2u32.pow(u32::from(target - 8));
			assert_eq!(b.level, target);
			assert_eq!(b.x_min()?, a.x_min()? * scale);
			assert_eq!(b.y_min()?, a.y_min()? * scale);
			assert_eq!(b.x_max()?, (a.x_max()? + 1) * scale - 1);
			assert_eq!(b.y_max()?, (a.y_max()? + 1) * scale - 1);
		} else {
			let scale = 2u32.pow(u32::from(8 - target));
			assert_eq!(b.level, target);
			assert_eq!(b.x_min()?, a.x_min()? / scale);
			assert_eq!(b.y_min()?, a.y_min()? / scale);
			assert_eq!(b.x_max()?, a.x_max()? / scale);
			assert_eq!(b.y_max()?, a.y_max()? / scale);
		}
		Ok(())
	}

	// ------------------------------ round / rounded ------------------------------

	/// Test that round aligns edges to multiples of block_size.
	#[rstest]
	// basic block sizes on a mid-level bbox
	#[case(6, 10, 10, 17, 21, 1, 10, 10, 17, 21)] // block=1 → no-op
	#[case(6, 10, 10, 17, 21, 2, 10, 10, 17, 21)] // already aligned to 2
	#[case(6, 10, 10, 17, 21, 4, 8, 8, 19, 23)] // expands to 4-boundaries
	#[case(6, 10, 10, 17, 21, 8, 8, 8, 23, 23)] // expands to 8-boundaries
	#[case(6, 10, 10, 17, 21, 16, 0, 0, 31, 31)] // expands to 16-boundaries
	#[case(6, 10, 10, 17, 21, 32, 0, 0, 31, 31)] // expands to 32-boundaries (== level extent at z6: 64 tiles, fits)
	#[case(6, 10, 10, 17, 21, 64, 0, 0, 63, 63)] // expands to 64-boundaries (exceeds level extent → clamps to level)
	#[case(6, 10, 10, 17, 21, 128, 0, 0, 63, 63)] // expands to 128-boundaries (exceeds level extent → clamps to level)
	// single-pixel bbox
	#[case(6, 0, 0, 0, 0, 4, 0, 0, 3, 3)] // single pixel at origin → expands to 4x4
	#[case(6, 3, 5, 3, 5, 4, 0, 4, 3, 7)] // single pixel mid-range → aligns to 4
	#[case(6, 63, 63, 63, 63, 4, 60, 60, 63, 63)] // single pixel at max_coord → stays at boundary
	// already aligned bbox → no change
	#[case(6, 0, 0, 31, 31, 32, 0, 0, 31, 31)]
	#[case(6, 32, 0, 63, 31, 32, 32, 0, 63, 31)]
	// bbox at level edge
	#[case(6, 48, 48, 63, 63, 16, 48, 48, 63, 63)] // top-right corner, aligned
	#[case(6, 50, 50, 63, 63, 16, 48, 48, 63, 63)] // top-right corner, not aligned → expands down
	// block_size equals max_count (whole level in one block)
	#[case(3, 2, 3, 5, 6, 8, 0, 0, 7, 7)] // z3 has 8 tiles; block=8 → full level
	#[case(2, 1, 1, 2, 2, 4, 0, 0, 3, 3)] // z2 has 4 tiles; block=4 → full level
	#[case(1, 0, 0, 1, 1, 2, 0, 0, 1, 1)] // z1 has 2 tiles; block=2 → full level
	// small levels
	#[case(0, 0, 0, 0, 0, 1, 0, 0, 0, 0)] // z0: single tile, block=1 → no-op
	#[case(1, 0, 0, 0, 0, 2, 0, 0, 1, 1)] // z1: single tile, block=2 → full level
	// asymmetric bbox
	#[case(6, 0, 10, 3, 21, 4, 0, 8, 3, 23)] // narrow in x, wider in y
	#[case(6, 10, 0, 21, 3, 4, 8, 0, 23, 3)] // wider in x, narrow in y
	fn round_edge_cases(
		#[case] level: u8,
		#[case] x0: u32,
		#[case] y0: u32,
		#[case] x1: u32,
		#[case] y1: u32,
		#[case] block: u32,
		#[case] exp_x0: u32,
		#[case] exp_y0: u32,
		#[case] exp_x1: u32,
		#[case] exp_y1: u32,
	) -> Result<()> {
		let mut b = bb(level, x0, y0, x1, y1);
		b.round(block);
		assert_eq!(b.to_array()?, [exp_x0, exp_y0, exp_x1, exp_y1]);
		Ok(())
	}

	/// Test that rounded() is a pure copy — original unchanged.
	#[test]
	fn rounded_is_pure() -> Result<()> {
		let original = bb(6, 10, 10, 17, 21);
		let rounded = original.rounded(8);
		assert_eq!(rounded.to_array()?, [8, 8, 23, 23]);
		// original is unchanged
		assert_eq!(original.to_array()?, [10, 10, 17, 21]);
		Ok(())
	}

	/// Test that round is a no-op on empty bboxes.
	#[test]
	fn round_noop_on_empty() -> Result<()> {
		let mut b = TileBBox::new_empty(6)?;
		b.round(4);
		assert!(b.is_empty());
		Ok(())
	}

	/// Test that round aligns all edges to block boundaries.
	#[rstest]
	#[case(1)]
	#[case(2)]
	#[case(4)]
	#[case(8)]
	#[case(16)]
	#[case(32)]
	fn round_edges_align_to_block(#[case] block: u32) -> Result<()> {
		// Use z8 (256 tiles) to have room for all block sizes
		let mut b = bb(8, 10, 10, 77, 99);
		b.round(block);
		if block > 1 {
			assert_eq!(b.x_min()? % block, 0, "x_min not aligned to {block}");
			assert_eq!(b.y_min()? % block, 0, "y_min not aligned to {block}");
			assert_eq!((b.x_max()? + 1) % block, 0, "x_max+1 not aligned to {block}");
			assert_eq!((b.y_max()? + 1) % block, 0, "y_max+1 not aligned to {block}");
		}
		// round never shrinks
		assert!(b.x_min()? <= 10);
		assert!(b.y_min()? <= 10);
		assert!(b.x_max()? >= 77);
		assert!(b.y_max()? >= 99);
		Ok(())
	}

	/// Test that round always contains the original bbox.
	#[rstest]
	#[case(6, 0, 0, 0, 0, 4)]
	#[case(6, 15, 15, 15, 15, 16)]
	#[case(8, 100, 200, 150, 250, 32)]
	#[case(8, 0, 0, 255, 255, 256)]
	fn round_always_contains_original(
		#[case] level: u8,
		#[case] x0: u32,
		#[case] y0: u32,
		#[case] x1: u32,
		#[case] y1: u32,
		#[case] block: u32,
	) -> Result<()> {
		let original = bb(level, x0, y0, x1, y1);
		let rounded = original.rounded(block);
		assert!(rounded.x_min()? <= original.x_min()?);
		assert!(rounded.y_min()? <= original.y_min()?);
		assert!(rounded.x_max()? >= original.x_max()?);
		assert!(rounded.y_max()? >= original.y_max()?);
		assert_eq!(rounded.level, original.level);
		Ok(())
	}

	// ------------------------------ flip_y ------------------------------
	#[test]
	fn flip_y_inverts_vertically() -> Result<()> {
		// For z=3, max_coord = 2^3 - 1 = 7
		let mut b = bb(3, 1, 2, 3, 4);
		b.flip_y();
		// y' = max_coord - y_max = 7 - 4 = 3; keep height=3 → y_min'=3, y_max'=5
		assert_eq!(b.to_array()?, [1, 3, 3, 5]);
		Ok(())
	}

	fn tc(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	#[test]
	fn include_tile() -> Result<()> {
		let mut bbox = TileBBox::from_min_and_max(4, 0, 1, 2, 3)?;
		bbox.insert_xy(4, 5);
		assert_eq!(bbox, TileBBox::from_min_and_max(4, 0, 1, 4, 5)?);
		Ok(())
	}

	#[test]
	fn add_border() -> Result<()> {
		let mut bbox = TileBBox::from_min_and_max(8, 5, 10, 20, 30)?;

		// Border of 1 should increase the size of the bbox by 1 in all directions
		bbox.buffer(1);
		assert_eq!(bbox, TileBBox::from_min_and_max(8, 4, 9, 21, 31)?);

		// Border of 0 should not change the size of the bbox
		bbox.buffer(0);
		assert_eq!(bbox, TileBBox::from_min_and_max(8, 4, 9, 21, 31)?);

		// Large border should saturate at max=255 for level=8
		bbox.buffer(999);
		assert_eq!(bbox, TileBBox::from_min_and_max(8, 0, 0, 255, 255)?);

		let mut bbox = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;

		// Attempt to add a border with zero values
		bbox.buffer(0);
		assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 15, 20)?);

		// Add a border that exceeds bounds, should clamp to max
		bbox.buffer(10);
		assert_eq!(bbox, TileBBox::from_min_and_max(6, 0, 0, 25, 30)?);

		// If bbox is empty, add_border should have no effect
		let mut empty_bbox = TileBBox::new_empty(8)?;
		empty_bbox.buffer(1);
		assert_eq!(empty_bbox, TileBBox::new_empty(8)?);

		Ok(())
	}

	#[test]
	fn test_shift_by() -> Result<()> {
		let mut bbox = TileBBox::from_min_and_max(4, 1, 2, 3, 4)?;
		bbox.shift_by(1, 1)?;
		assert_eq!(bbox, TileBBox::from_min_and_max(4, 2, 3, 4, 5)?);
		Ok(())
	}

	#[test]
	fn test_include_tile() -> Result<()> {
		let mut bbox = TileBBox::from_min_and_max(6, 5, 10, 20, 30)?;
		bbox.insert_xy(25, 35);
		assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 25, 35)?);
		Ok(())
	}

	#[test]
	fn test_include_bbox() -> Result<()> {
		let mut bbox1 = TileBBox::from_min_and_max(4, 0, 11, 2, 13)?;
		let bbox2 = TileBBox::from_min_and_max(4, 1, 10, 3, 12)?;
		bbox1.insert_bbox(&bbox2)?;
		assert_eq!(bbox1, TileBBox::from_min_and_max(4, 0, 10, 3, 13)?);
		Ok(())
	}

	#[test]
	fn test_include() -> Result<()> {
		let mut bbox = TileBBox::new_empty(6)?;
		bbox.insert_xy(5, 10);
		assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 5, 10)?);

		bbox.insert_xy(15, 20);
		assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 15, 20)?);

		bbox.insert_xy(10, 15);
		assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 15, 20)?);

		Ok(())
	}

	#[test]
	fn test_include_coord() -> Result<()> {
		let mut bbox = TileBBox::new_empty(6)?;
		let coord = tc(6, 5, 10);
		bbox.insert_coord(&coord)?;
		assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 5, 10)?);

		let coord = tc(6, 15, 20);
		bbox.insert_coord(&coord)?;
		assert_eq!(bbox, TileBBox::from_min_and_max(6, 5, 10, 15, 20)?);

		// Attempt to include a coordinate with a different zoom level
		let coord_invalid = tc(5, 10, 15);
		let result = bbox.insert_coord(&coord_invalid);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_include_bbox_correctly_with_valid_and_empty_bboxes() -> Result<()> {
		let mut bbox1 = TileBBox::from_min_and_max(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::from_min_and_max(6, 10, 15, 20, 25)?;

		bbox1.insert_bbox(&bbox2)?;
		assert_eq!(bbox1, TileBBox::from_min_and_max(6, 5, 10, 20, 25)?);

		// Including an empty bounding box should have no effect
		let empty_bbox = TileBBox::new_empty(6)?;
		bbox1.insert_bbox(&empty_bbox)?;
		assert_eq!(bbox1, TileBBox::from_min_and_max(6, 5, 10, 20, 25)?);

		// Attempting to include a bounding box with different zoom level
		let bbox_diff_level = TileBBox::from_min_and_max(5, 5, 10, 20, 25)?;
		let result = bbox1.insert_bbox(&bbox_diff_level);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_scale_down_correctly() -> Result<()> {
		let mut bbox = TileBBox::from_min_and_max(4, 4, 4, 7, 7)?;
		bbox.scale_down(2);
		assert_eq!(bbox, TileBBox::from_min_and_max(4, 2, 2, 3, 3)?);

		// Scaling down by a factor larger than the coordinates
		bbox.scale_down(4);
		assert_eq!(bbox, TileBBox::from_min_and_max(4, 0, 0, 0, 0)?);

		Ok(())
	}

	#[test]
	fn test_scaled_down_returns_new_bbox_and_preserves_original() -> Result<()> {
		let original = TileBBox::from_min_and_max(5, 10, 15, 20, 25)?;
		let scaled = original.scaled_down(4);
		assert_eq!(scaled, TileBBox::from_min_and_max(5, 2, 3, 5, 6)?);
		assert_eq!(original, TileBBox::from_min_and_max(5, 10, 15, 20, 25)?);
		let same = original.scaled_down(1);
		assert_eq!(same, original);
		Ok(())
	}

	#[rstest]
	#[case((0, 11, 0, 2))]
	#[case((1, 12, 0, 3))]
	#[case((2, 13, 0, 3))]
	#[case((3, 14, 0, 3))]
	#[case((4, 15, 1, 3))]
	#[case((5, 16, 1, 4))]
	#[case((6, 17, 1, 4))]
	#[case((7, 18, 1, 4))]
	#[case((8, 19, 2, 4))]
	fn test_scale_down_cases(#[case] args: (u32, u32, u32, u32)) -> Result<()> {
		let (min0, max0, min1, max1) = args;
		let mut bbox0 = TileBBox::from_min_and_max(8, min0, min0, max0, max0)?;
		let bbox1 = TileBBox::from_min_and_max(8, min1, min1, max1, max1)?;
		assert_eq!(
			bbox0.scaled_down(4),
			bbox1,
			"scaled_down(4) of {bbox0:?} should return {bbox1:?}"
		);
		bbox0.scale_down(4);
		assert_eq!(bbox0, bbox1, "scale_down(4) of {bbox0:?} should result in {bbox1:?}");
		Ok(())
	}

	#[test]
	fn should_shift_bbox_correctly() -> Result<()> {
		let mut bbox = TileBBox::from_min_and_size(6, 5, 10, 10, 10)?;
		bbox.shift_by(3, 4)?;
		assert_eq!(bbox, TileBBox::from_min_and_size(6, 8, 14, 10, 10)?);

		// Shifting beyond max should not cause overflow due to saturating_add
		let mut bbox = TileBBox::from_min_and_size(6, 14, 14, 10, 10)?;
		bbox.shift_by(2, 2)?;
		assert_eq!(bbox, TileBBox::from_min_and_size(6, 16, 16, 10, 10)?);

		let mut bbox = TileBBox::from_min_and_size(6, 5, 10, 10, 10)?;
		bbox.shift_by(-3, -5)?;
		assert_eq!(bbox, TileBBox::from_min_and_size(6, 2, 5, 10, 10)?);

		// Subtracting more than current coordinates should saturate at 0
		bbox.shift_by(-5, -10)?;
		assert_eq!(bbox, TileBBox::from_min_and_size(6, 0, 0, 10, 10)?);

		Ok(())
	}

	#[rstest]
	#[case([1, 2, 16, 17], [0, 0, 19, 19])]
	#[case([2, 3, 17, 18], [0, 0, 19, 19])]
	#[case([3, 4, 18, 19], [0, 4, 19, 19])]
	#[case([4, 5, 19, 20], [4, 4, 19, 23])]
	#[case([5, 6, 20, 21], [4, 4, 23, 23])]
	#[case([6, 7, 21, 22], [4, 4, 23, 23])]
	#[case([7, 8, 22, 23], [4, 8, 23, 23])]
	#[case([8, 9, 23, 24], [8, 8, 23, 27])]
	fn test_round_shifting_cases(#[case] inp: [u32; 4], #[case] exp: [u32; 4]) -> Result<()> {
		let bbox_exp = TileBBox::from_min_and_max(8, exp[0], exp[1], exp[2], exp[3])?;
		let mut bbox_inp = TileBBox::from_min_and_max(8, inp[0], inp[1], inp[2], inp[3])?;
		assert_eq!(bbox_inp.rounded(4), bbox_exp);
		bbox_inp.round(4);
		assert_eq!(bbox_inp, bbox_exp);
		Ok(())
	}

	#[rstest]
	#[case(1, [12, 34, 56, 78])]
	#[case(2, [12, 34, 57, 79])]
	#[case(3, [12, 33, 56, 80])]
	#[case(4, [12, 32, 59, 79])]
	#[case(5, [10, 30, 59, 79])]
	#[case(6, [12, 30, 59, 83])]
	#[case(7, [7, 28, 62, 83])]
	#[case(10, [10, 30, 59, 79])]
	#[case(100, [0, 0, 99, 99])]
	#[case(1024, [0, 0, 1023, 1023])]
	fn test_round_scaling_cases(#[case] scale: u32, #[case] exp: [u32; 4]) -> Result<()> {
		let bbox_exp = TileBBox::from_min_and_max(12, exp[0], exp[1], exp[2], exp[3])?;
		let mut bbox_inp = TileBBox::from_min_and_max(12, 12, 34, 56, 78)?;
		assert_eq!(bbox_inp.rounded(scale), bbox_exp);
		bbox_inp.round(scale);
		assert_eq!(bbox_inp, bbox_exp);
		Ok(())
	}

	#[rstest]
	#[case((1, 0, 0, 1, 1), (1, 0, 0, 1, 1))]
	#[case((2, 0, 0, 1, 1), (2, 0, 2, 1, 3))]
	#[case((3, 0, 0, 1, 1), (3, 0, 6, 1, 7))]
	#[case((9, 10, 0, 10, 511), (9, 10, 0, 10, 511))]
	#[case((9, 0, 10, 511, 10), (9, 0, 501, 511, 501))]
	fn bbox_flip_y(#[case] a: (u8, u32, u32, u32, u32), #[case] b: (u8, u32, u32, u32, u32)) -> Result<()> {
		let mut t = TileBBox::from_min_and_max(a.0, a.1, a.2, a.3, a.4)?;
		t.flip_y();

		assert_eq!(t, TileBBox::from_min_and_max(b.0, b.1, b.2, b.3, b.4)?);
		Ok(())
	}

	#[rstest]
	#[case(4, 6, 2, 3)]
	#[case(5, 6, 2, 3)]
	#[case(4, 7, 2, 3)]
	#[case(5, 7, 2, 3)]
	fn level_decrease(
		#[case] min_in: u32,
		#[case] max_in: u32,
		#[case] min_out: u32,
		#[case] max_out: u32,
	) -> Result<()> {
		let mut bbox = TileBBox::from_min_and_max(10, min_in, min_in, max_in, max_in)?;
		bbox.level_down();
		assert_eq!(bbox.level, 9);
		assert_eq!(bbox.to_array()?, [min_out, min_out, max_out, max_out]);
		Ok(())
	}

	#[rstest]
	#[case(4, 6, 8, 13)]
	#[case(5, 6, 10, 13)]
	#[case(4, 7, 8, 15)]
	#[case(5, 7, 10, 15)]
	fn level_increase(
		#[case] min_in: u32,
		#[case] max_in: u32,
		#[case] min_out: u32,
		#[case] max_out: u32,
	) -> Result<()> {
		let mut bbox = TileBBox::from_min_and_max(10, min_in, min_in, max_in, max_in)?;
		bbox.level_up();
		assert_eq!(bbox.level, 11);
		assert_eq!(bbox.to_array()?, [min_out, min_out, max_out, max_out]);
		Ok(())
	}

	#[test]
	fn level_increase_decrease_roundtrip() -> Result<()> {
		let original = TileBBox::from_min_and_max(4, 5, 6, 7, 8)?;
		let inc = original.leveled_up();
		assert_eq!(inc.level, 5);
		assert_eq!(inc.to_array()?, [10, 12, 15, 17]);
		let dec = inc.leveled_down();
		assert_eq!(dec, original);
		Ok(())
	}

	#[rstest]
	#[case(0, 0, 0, 0, 0)]
	#[case(4, 0, 7, 8, 15)]
	#[case(5, 0, 15, 16, 31)]
	#[case(6, 0, 31, 32, 63)]
	#[case(7, 0, 62, 65, 127)]
	#[case(8, 0, 124, 131, 255)]
	#[case(10, 0, 496, 527, 1023)]
	#[case(20, 0, 507904, 540671, 1048575)]
	#[case(30, 0, 520093696, 553648127, 1073741823)]
	fn as_level_up_and_down(
		#[case] level: u32,
		#[case] x0: u32,
		#[case] y0: u32,
		#[case] x1: u32,
		#[case] y1: u32,
	) -> Result<()> {
		let bbox = TileBBox::from_min_and_max(6, 0, 31, 32, 63)?;
		let up = bbox.at_level(u8::try_from(level).unwrap());
		assert_eq!(
			[u32::from(up.level), up.x_min()?, up.y_min()?, up.x_max()?, up.y_max()?],
			[level, x0, y0, x1, y1]
		);
		Ok(())
	}

	#[rstest]
	#[case((5, 5, 10, 7, 12), 2, (5, 10, 20, 15, 25))]
	#[case((4, 1, 1, 2, 2), 4, (4, 4, 4, 11, 11))]
	#[case((8, 0, 0, 0, 0), 8, (8, 0, 0, 7, 7))]
	#[case((6, 3, 5, 3, 5), 2, (6, 6, 10, 7, 11))]
	fn test_scaled_up_cases(
		#[case] input: (u8, u32, u32, u32, u32),
		#[case] scale: u32,
		#[case] expected: (u8, u32, u32, u32, u32),
	) -> Result<()> {
		let (level, x0, y0, x1, y1) = input;
		let bbox = TileBBox::from_min_and_max(level, x0, y0, x1, y1)?;
		let scaled = bbox.scaled_up(scale)?;
		let (exp_level, exp_x0, exp_y0, exp_x1, exp_y1) = expected;
		assert_eq!(scaled.level, exp_level);
		assert_eq!(scaled.to_array()?, [exp_x0, exp_y0, exp_x1, exp_y1]);
		// Ensure original bbox remains unchanged
		assert_eq!(bbox, TileBBox::from_min_and_max(level, x0, y0, x1, y1)?);
		Ok(())
	}

	// ── swap_xy involution: (x, y) ↔ (y, x) applied twice is identity ────────
	#[rstest]
	#[case(bb(3, 1, 2, 3, 4))]
	#[case(bb(4, 0, 0, 15, 15))] // full
	#[case(bb(5, 0, 0, 0, 0))] // single tile
	#[case(TileBBox::new_empty(3).unwrap())]
	fn swap_xy_is_involution(#[case] original: TileBBox) {
		let mut b = original;
		b.swap_xy();
		b.swap_xy();
		assert_eq!(b, original);
	}

	#[rstest]
	#[case(bb(4, 1, 2, 3, 4), [2, 1, 4, 3])]
	#[case(bb(4, 0, 0, 0, 0), [0, 0, 0, 0])]
	#[case(bb(4, 5, 10, 5, 10), [10, 5, 10, 5])]
	fn swap_xy_swaps_axes(#[case] input: TileBBox, #[case] expected: [u32; 4]) -> Result<()> {
		let mut b = input;
		b.swap_xy();
		assert_eq!(b.to_array()?, expected);
		Ok(())
	}

	// ── flip_y involution ────────────────────────────────────────────────────
	#[rstest]
	#[case(bb(3, 1, 2, 3, 4))]
	#[case(bb(4, 0, 0, 15, 15))]
	#[case(bb(4, 0, 0, 0, 0))]
	#[case(TileBBox::new_empty(3).unwrap())]
	fn flip_y_is_involution(#[case] original: TileBBox) {
		let mut b = original;
		b.flip_y();
		b.flip_y();
		assert_eq!(b, original);
	}

	// ── Identity operations ──────────────────────────────────────────────────
	#[rstest]
	#[case(bb(4, 1, 2, 3, 4))]
	#[case(TileBBox::new_empty(4).unwrap())]
	#[case(TileBBox::new_full(4).unwrap())]
	fn scaled_up_by_one_is_identity(#[case] input: TileBBox) -> Result<()> {
		let out = input.scaled_up(1)?;
		assert_eq!(out, input);
		Ok(())
	}

	#[rstest]
	#[case(bb(4, 1, 2, 3, 4))]
	#[case(TileBBox::new_empty(4).unwrap())]
	fn at_level_same_level_is_identity(#[case] input: TileBBox) {
		assert_eq!(input.at_level(input.level), input);
	}

	#[rstest]
	#[case(bb(4, 1, 2, 3, 4))]
	#[case(bb(4, 0, 0, 15, 15))]
	#[case(TileBBox::new_empty(4).unwrap())]
	fn rounded_by_one_is_identity(#[case] input: TileBBox) {
		assert_eq!(input.rounded(1), input);
	}

	// ── level_up then level_down round-trip ─────────────────────────────────
	#[rstest]
	#[case(bb(4, 0, 0, 0, 0))]
	#[case(bb(4, 2, 4, 6, 10))]
	#[case(bb(10, 100, 200, 300, 500))]
	fn level_up_then_down_roundtrip(#[case] input: TileBBox) -> Result<()> {
		let mut b = input;
		b.level_up();
		b.level_down();
		// Going up-down should return to the original (or a superset, but for even coords it's exact).
		assert!(
			b.includes_bbox(&input),
			"level_up then level_down should preserve or enlarge coverage"
		);
		assert_eq!(b.level, input.level);
		Ok(())
	}

	// ── scale_up rejects zero and 1 is a no-op ──────────────────────────────
	#[rstest]
	#[case(0)]
	fn scale_up_rejects_invalid_factors(#[case] factor: u32) {
		let mut b = bb(4, 0, 0, 1, 1);
		assert!(b.scale_up(factor).is_err());
	}

	#[test]
	fn scale_up_by_one_is_identity() -> Result<()> {
		let mut b = bb(4, 3, 5, 7, 9);
		let before = b;
		b.scale_up(1)?;
		assert_eq!(b, before);
		Ok(())
	}
}
