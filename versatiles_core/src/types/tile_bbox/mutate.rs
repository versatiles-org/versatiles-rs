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

use crate::{MAX_ZOOM_LEVEL, TileBBox, TileCoord, TilePyramid, validate_zoom_level};
use anyhow::{Result, ensure};
use std::ops::Div;
use versatiles_derive::context;

impl TileBBox {
	/// Include a specific tile coordinate `(x, y)` into this bbox.
	///
	/// If the bbox is empty, it becomes the single-tile bbox at `(x, y)`.
	/// Otherwise, the bbox is expanded minimally to include the coordinate.
	///
	/// # Panics
	/// Panics if `x` or `y` are out of range for the current level.
	pub fn include(&mut self, x: u32, y: u32) {
		assert!(x < self.max_count(), "x ({x}) must be < max ({})", self.max_count());
		assert!(y < self.max_count(), "y ({y}) must be < max ({})", self.max_count());
		if self.is_empty() {
			// Initialize bounding box to the provided coordinate
			self.set_min_and_size(x, y, 1, 1).unwrap();
		} else {
			// Expand bounding box to include the new coordinate
			if x < self.x_min().unwrap() {
				self.set_x_min(x).unwrap();
			} else if x > self.x_max().unwrap() {
				self.set_x_max(x).unwrap();
			}
			if y < self.y_min().unwrap() {
				self.set_y_min(y).unwrap();
			} else if y > self.y_max().unwrap() {
				self.set_y_max(y).unwrap();
			}
		}
	}

	/// Includes a tile coordinate (`TileCoord`) within the bounding box.
	///
	/// Expands the bounding box to encompass the given coordinate. The zoom level of the coordinate
	/// must match the bounding box's zoom level.
	///
	/// # Arguments
	///
	/// * `coord` - Reference to the tile coordinate to include.
	///
	/// # Returns
	///
	/// * `Ok(())` if inclusion is successful.
	/// * `Err(anyhow::Error)` if the zoom levels do not match or other validations fail.
	#[context("Failed to include TileCoord {coord:?} into TileBBox {self:?}")]
	pub fn include_coord(&mut self, coord: &TileCoord) -> Result<()> {
		ensure!(
			coord.level == self.level,
			"Cannot include TileCoord with z={} into TileBBox at z={}",
			coord.level,
			self.level
		);
		self.include(coord.x, coord.y);
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
					self.x_min().unwrap().saturating_sub(size),
					self.y_min().unwrap().saturating_sub(size),
					self.x_max().unwrap().saturating_add(size).min(max),
					self.y_max().unwrap().saturating_add(size).min(max),
				)
				.unwrap();
		}
	}

	/// Expands the bounding box to include another bounding box.
	///
	/// Merges the extents of `bbox` into this bounding box. Both bounding boxes must be at the same zoom level.
	///
	/// # Arguments
	///
	/// * `bbox` - Reference to the `TileBBox` to include.
	///
	/// # Returns
	///
	/// * `Ok(())` if inclusion is successful.
	/// * `Err(anyhow::Error)` if the zoom levels do not match or other validations fail.
	#[context("Failed to include TileBBox {bbox:?} into TileBBox {self:?}")]
	pub fn include_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
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
				self.x_min().unwrap().min(bbox.x_min().unwrap()),
				self.y_min().unwrap().min(bbox.y_min().unwrap()),
				self.x_max().unwrap().max(bbox.x_max().unwrap()),
				self.y_max().unwrap().max(bbox.y_max().unwrap()),
			)?;
		}

		Ok(())
	}

	/// Intersects the bounding box with another bounding box.
	///
	/// Modifies this bounding box to represent the overlapping area with `bbox`.
	/// Both bounding boxes must be at the same zoom level.
	///
	/// # Arguments
	///
	/// * `bbox` - Reference to the `TileBBox` to intersect with.
	///
	/// # Returns
	///
	/// * `Ok(())` if intersection is successful.
	/// * `Err(anyhow::Error)` if the zoom levels do not match or other validations fail.
	#[context("Failed to intersect TileBBox {self:?} with TileBBox {bbox:?}")]
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		ensure!(
			self.level == bbox.level,
			"Cannot intersect TileBBox at zoom level {} with TileBBox at zoom level {}",
			bbox.level,
			self.level
		);

		if self.is_empty() || bbox.is_empty() {
			self.set_empty();
			return Ok(());
		}

		let x_min = self.x_min()?.max(bbox.x_min()?);
		let y_min = self.y_min()?.max(bbox.y_min()?);
		let x_max = self.x_max()?.min(bbox.x_max()?);
		let y_max = self.y_max()?.min(bbox.y_max()?);

		if x_min > x_max || y_min > y_max {
			self.set_empty();
		} else {
			self.set_min_and_max(x_min, y_min, x_max, y_max)?;
		}

		Ok(())
	}

	/// Intersect this bbox with the coverage of a [`TilePyramid`] at this bbox’s zoom level.
	///
	/// If the pyramid has no tiles at this zoom level, the bbox is set to empty.
	pub fn intersect_with_pyramid(&mut self, pyramid: &TilePyramid) {
		if let Some(level_bbox) = pyramid.get_level(self.level).bounds() {
			self.intersect_bbox(&level_bbox).unwrap_or(());
		} else {
			self.set_empty();
		}
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
			self.height().min(self.max_coord() - y_min),
		)
	}

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
				self.x_min().unwrap() / scale,
				self.y_min().unwrap() / scale,
				self.x_max().unwrap() / scale,
				self.y_max().unwrap() / scale,
			)
			.unwrap();
	}

	/// Return a downscaled **copy** of this bbox by an integer power-of-two factor.
	#[must_use]
	pub fn scaled_down(&self, scale: u32) -> TileBBox {
		if self.is_empty() {
			return TileBBox::new_empty(self.level).unwrap();
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
		self.scale_up(2).unwrap();
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
		validate_zoom_level(level).unwrap();

		let mut bbox = *self;
		if level > self.level {
			let scale = 2u32.pow(u32::from(level - self.level));
			bbox.level = level;
			bbox.scale_up(scale).unwrap();
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
				(self.x_min().unwrap().div(block_size)) * block_size,
				(self.y_min().unwrap().div(block_size)) * block_size,
				((self.x_max().unwrap() + 1).div_ceil(block_size) * block_size - 1).min(max_coord),
				((self.y_max().unwrap() + 1).div_ceil(block_size) * block_size - 1).min(max_coord),
			)
			.unwrap();
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
				.shift_to(self.x_min().unwrap(), self.max_coord() - self.y_max().unwrap())
				.unwrap();
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
		b.include(3, 5);
		assert_eq!(b.as_array()?, [3, 5, 3, 5]);

		b.include(6, 2); // expand both axes
		assert_eq!(b.as_array()?, [3, 2, 6, 5]);
		Ok(())
	}

	#[test]
	#[should_panic(expected = "x (16) must be < max (16)")]
	fn include_panics_on_out_of_bounds_x() {
		let mut b = TileBBox::new_empty(4).unwrap(); // max_count=16
		b.include(16, 0);
	}

	#[test]
	fn include_coord_level_mismatch_errors() -> Result<()> {
		let mut b = bb(5, 10, 10, 12, 12);
		let tc = TileCoord::new(6, 11, 11)?; // different level
		assert!(b.include_coord(&tc).is_err());
		Ok(())
	}

	// ------------------------------ expand_by ------------------------------
	#[rstest]
	#[case(bb(3, 1, 1, 2, 2), 1, [0,0,3,3])] // expand in all directions, clamp at bounds
	#[case(bb(3, 6, 6, 7, 7), 5, [1,1,7,7])] // subtract saturates at 0
	#[case(bb(3, 2, 2, 3, 3), 0, [2,2,3,3])] // no-op
	fn expand_by_behaviour(#[case] mut b: TileBBox, #[case] size: u32, #[case] expected: [u32; 4]) {
		b.buffer(size);
		assert_eq!(b.as_array().unwrap(), expected);
	}

	#[test]
	fn expand_by_noop_on_empty() {
		let mut b = TileBBox::new_empty(5).unwrap();
		b.buffer(10);
		assert!(b.is_empty());
	}

	// ------------------------------ include_bbox ------------------------------
	#[test]
	fn include_bbox_merges_ranges_and_ignores_empty() -> Result<()> {
		let mut a = bb(4, 4, 4, 6, 6);
		let b = bb(4, 2, 5, 8, 7);
		a.include_bbox(&b)?;
		assert_eq!(a.as_array()?, [2, 4, 8, 7]);
		let c = TileBBox::new_empty(4)?;
		a.include_bbox(&c)?; // no change
		assert_eq!(a.as_array()?, [2, 4, 8, 7]);
		Ok(())
	}

	#[test]
	fn include_bbox_level_mismatch_errors() -> Result<()> {
		let mut a = bb(3, 0, 0, 1, 1);
		let b = bb(4, 0, 0, 1, 1);
		assert!(a.include_bbox(&b).is_err());
		Ok(())
	}

	// ------------------------------ intersect_with / intersect_with_pyramid ------------------------------
	#[rstest]
	#[case(bb(5, 10,10, 20,20), bb(5, 15,15, 25,25), [15,15,20,20])] // partial overlap
	#[case(bb(5, 10,10, 20,20), bb(5, 0,0, 5,5),     [0,0,0,0])] // no overlap → empty
	#[case(bb(5, 10,10, 20,20), bb(5, 10,10, 20,20), [10,10,20,20])] // identical
	fn intersect_cases(#[case] mut a: TileBBox, #[case] b: TileBBox, #[case] exp: [u32; 4]) -> Result<()> {
		a.intersect_bbox(&b)?;
		if exp == [0, 0, 0, 0] && (a.width() == 0 || a.height() == 0) {
			// empty expected; nothing more to assert
			return Ok(());
		}
		assert_eq!(a.as_array()?, exp);
		Ok(())
	}

	#[test]
	fn intersect_with_pyramid_shrinks() -> Result<()> {
		use crate::TilePyramid;
		let full = TileBBox::new_full(5)?;
		let pyramid = TilePyramid::from([full].as_slice());
		let mut b = bb(5, 12, 12, 20, 20);
		b.intersect_with_pyramid(&pyramid);
		assert_eq!(b, bb(5, 12, 12, 20, 20)); // full pyramid covers all

		let small = bb(5, 14, 15, 16, 18);
		let py_small = TilePyramid::from([small].as_slice());
		let mut b2 = bb(5, 12, 12, 20, 20);
		b2.intersect_with_pyramid(&py_small);
		assert_eq!(b2, small);
		Ok(())
	}

	// ------------------------------ shift_by / shift_to ------------------------------
	#[test]
	fn shift_by_and_to() -> Result<()> {
		let mut b = bb(8, 5, 6, 7, 8); // 3x3
		b.shift_by(-5, -5)?; // clamp to 0
		assert_eq!(b.as_array()?, [0, 1, 2, 3]);
		b.shift_to(13, 14)?; // move within bounds
		assert_eq!(b.as_array()?, [13, 14, 15, 16]);
		Ok(())
	}

	// ------------------------------ scale down / up (and copies) ------------------------------
	#[test]
	fn scale_down_by_powers_of_two() -> Result<()> {
		let mut b = bb(5, 8, 10, 15, 17); // 8x8 region
		b.scale_down(2);
		assert_eq!(b.as_array()?, [4, 5, 7, 8]);
		b.scale_down(2);
		assert_eq!(b.as_array()?, [2, 2, 3, 4]);
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
		assert_eq!(c.as_array()?, [2, 2, 3, 4]);
		// Original unchanged
		assert_eq!(b, bb(5, 8, 10, 15, 17));
		Ok(())
	}

	#[test]
	fn scale_up_and_scaled_up() -> Result<()> {
		let mut b = bb(6, 2, 3, 4, 5); // 3x3
		b.scale_up(2)?;
		assert_eq!(b.as_array()?, [4, 6, 9, 11]);
		let c = b.scaled_up(2)?;
		assert_eq!(c.as_array()?, [8, 12, 19, 23]);
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
		assert_eq!(b.as_array()?, [2, 2, 5, 5]);
		b.level_down(); // back to z=2
		assert_eq!(b.level, 2);
		assert_eq!(b.as_array()?, [1, 1, 2, 2]);

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
		assert_eq!(b.as_array()?, [exp_x0, exp_y0, exp_x1, exp_y1]);
		Ok(())
	}

	/// Test that rounded() is a pure copy — original unchanged.
	#[test]
	fn rounded_is_pure() -> Result<()> {
		let original = bb(6, 10, 10, 17, 21);
		let rounded = original.rounded(8);
		assert_eq!(rounded.as_array()?, [8, 8, 23, 23]);
		// original is unchanged
		assert_eq!(original.as_array()?, [10, 10, 17, 21]);
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
		assert_eq!(b.as_array()?, [1, 3, 3, 5]);
		Ok(())
	}
}
