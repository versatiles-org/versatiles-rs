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

use crate::{TileBBox, TileBBoxPyramid, TileCoord};
use anyhow::{Result, ensure};
use versatiles_derive::context;

impl TileBBox {
	/// Include a specific tile coordinate `(x, y)` into this bbox.
	///
	/// If the bbox is empty, it becomes the single-tile bbox at `(x, y)`.
	/// Otherwise, the bbox is expanded minimally to include the coordinate.
	///
	/// # Panics
	/// Panics if `x` or `y` are out of range for the current level.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let mut bb = TileBBox::new_empty(4).unwrap();
	/// bb.include(3, 5);
	/// assert_eq!(bb.as_array(), [3,5,3,5]);
	/// ```
	pub fn include(&mut self, x: u32, y: u32) {
		assert!(x < self.max_count(), "x ({x}) must be < max ({})", self.max_count());
		assert!(y < self.max_count(), "y ({y}) must be < max ({})", self.max_count());
		if self.is_empty() {
			// Initialize bounding box to the provided coordinate
			self.set_min_and_size(x, y, 1, 1).unwrap();
		} else {
			// Expand bounding box to include the new coordinate
			if x < self.x_min() {
				self.set_x_min(x).unwrap();
			} else if x > self.x_max() {
				self.set_x_max(x).unwrap();
			}
			if y < self.y_min() {
				self.set_y_min(y).unwrap();
			} else if y > self.y_max() {
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
	///
	/// # Example
	/// ```
	/// # use versatiles_core::{TileBBox, TileCoord};
	/// let mut bb = TileBBox::new_empty(5).unwrap();
	/// bb.include_coord(&TileCoord::new(5, 7, 9).unwrap()).unwrap();
	/// assert_eq!(bb.as_array(), [7,9,7,9]);
	/// ```
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

	/// Add a border to the bbox by expanding its min/max.
	///
	/// Subtracts `(x_min, y_min)` from the current minimum and adds `(x_max, y_max)`
	/// to the current maximum. The expansion is **clamped** to the level’s bounds.
	///
	/// This method is infallible and a no-op for empty bboxes.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let mut bb = TileBBox::from_min_and_max(3, 2, 2, 3, 3).unwrap();
	/// bb.expand_by(1, 1, 2, 2);
	/// assert_eq!(bb.as_array(), [1,1,5,5]);
	/// ```
	pub fn expand_by(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) {
		if !self.is_empty() {
			let max = self.max_count() - 1;
			self
				.set_min_and_max(
					self.x_min().saturating_sub(x_min),
					self.y_min().saturating_sub(y_min),
					self.x_max().saturating_add(x_max).min(max),
					self.y_max().saturating_add(y_max).min(max),
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
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let mut a = TileBBox::from_min_and_max(4, 4, 4, 6, 6).unwrap();
	/// let b = TileBBox::from_min_and_max(4, 2, 5, 8, 7).unwrap();
	/// a.include_bbox(&b).unwrap();
	/// assert_eq!(a.as_array(), [2,4,8,7]);
	/// ```
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
				self.x_min().min(bbox.x_min()),
				self.y_min().min(bbox.y_min()),
				self.x_max().max(bbox.x_max()),
				self.y_max().max(bbox.y_max()),
			)?
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
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let mut a = TileBBox::from_min_and_max(5, 10,10, 20,20).unwrap();
	/// let b = TileBBox::from_min_and_max(5, 15,15, 25,25).unwrap();
	/// a.intersect_with(&b).unwrap();
	/// assert_eq!(a.as_array(), [15,15,20,20]);
	/// ```
	#[context("Failed to intersect TileBBox {self:?} with TileBBox {bbox:?}")]
	pub fn intersect_with(&mut self, bbox: &TileBBox) -> Result<()> {
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

		let x_min = self.x_min().max(bbox.x_min());
		let y_min = self.y_min().max(bbox.y_min());
		let x_max = self.x_max().min(bbox.x_max());
		let y_max = self.y_max().min(bbox.y_max());

		if x_min > x_max || y_min > y_max {
			self.set_empty();
		} else {
			self.set_min_and_max(x_min, y_min, x_max, y_max)?;
		}

		Ok(())
	}

	/// Intersect with the pyramid’s bbox at this bbox’s zoom level.
	///
	/// Equivalent to `self.intersect_with(pyramid.get_level_bbox(self.level))`.
	pub fn intersect_with_pyramid(&mut self, pyramid: &TileBBoxPyramid) {
		self.intersect_with(pyramid.get_level_bbox(self.level)).unwrap();
	}

	/// Shift the bbox by integer offsets `(dx, dy)`.
	///
	/// Negative shifts are **clamped** at zero; the bbox never moves outside the
	/// valid range for its level.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let mut bb = TileBBox::from_min_and_max(4, 5, 6, 7, 8).unwrap();
	/// bb.shift_by(-10, -10).unwrap();
	/// assert_eq!(bb.as_array(), [0,0,2,2]);
	/// ```
	#[context("Failed to shift TileBBox {self:?} by ({x}, {y})")]
	pub fn shift_by(&mut self, x: i64, y: i64) -> Result<()> {
		self.shift_to(
			(i64::from(self.x_min()) + x).max(0) as u32,
			(i64::from(self.y_min()) + y).max(0) as u32,
		)
	}

	#[context("Failed to shift TileBBox {self:?} to ({x_min}, {y_min})")]
	pub fn shift_to(&mut self, x_min: u32, y_min: u32) -> Result<()> {
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
		assert!(scale > 0, "scale must be greater than 0");
		assert!(scale.is_power_of_two(), "scale must be a power of two");

		self
			.set_min_and_max(
				self.x_min() / scale,
				self.y_min() / scale,
				self.x_max() / scale,
				self.y_max() / scale,
			)
			.unwrap();
	}

	/// Return a downscaled **copy** of this bbox by an integer power-of-two factor.
	pub fn scaled_down(&self, scale: u32) -> TileBBox {
		let mut bbox = *self;
		bbox.scale_down(scale);
		bbox
	}

	/// Scale coordinates by an integer factor (≥ 1).
	///
	/// Expands `(x_max, y_max)` to keep the same **inclusive** extent semantics.
	#[context("Failed to scale up TileBBox {self:?} by factor {scale}")]
	pub fn scale_up(&mut self, scale: u32) -> Result<()> {
		ensure!(scale > 0, "scale must be greater than 0");

		self.set_min_and_max(
			self.x_min() * scale,
			self.y_min() * scale,
			(self.x_max() + 1) * scale - 1,
			(self.y_max() + 1) * scale - 1,
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
		assert!(self.level < 31, "level must be less than 31");
		self.level += 1;
		self.scale_up(2).unwrap()
	}

	/// Decrease the zoom level by one and divide coordinates by 2.
	pub fn level_down(&mut self) {
		assert!(self.level > 0, "level must be greater than 0");
		self.level -= 1;
		self.scale_down(2)
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
		assert!(level <= 31, "level ({level}) must be <= 31");

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
	pub fn round(&mut self, block_size: u32) {
		self
			.set_min_and_max(
				(self.x_min() / block_size) * block_size,
				(self.y_min() / block_size) * block_size,
				(self.x_max() + 1).div_ceil(block_size) * block_size - 1,
				(self.y_max() + 1).div_ceil(block_size) * block_size - 1,
			)
			.unwrap()
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
			self.shift_to(self.x_min(), self.max_coord() - self.y_max()).unwrap();
		}
	}
}

#[cfg(test)]
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
		assert_eq!(b.as_array(), [3, 5, 3, 5]);

		b.include(6, 2); // expand both axes
		assert_eq!(b.as_array(), [3, 2, 6, 5]);
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
	#[case(bb(3, 1, 1, 2, 2), [1,1,1,1], [0,0,3,3])] // expand in all directions, clamp at bounds
	#[case(bb(3, 6, 6, 7, 7), [5,5,5,5], [1,1,7,7])] // subtract saturates at 0
	#[case(bb(3, 2, 2, 3, 3), [0,0,0,0], [2,2,3,3])] // no-op
	fn expand_by_behaviour(#[case] mut b: TileBBox, #[case] off: [u32; 4], #[case] expected: [u32; 4]) {
		b.expand_by(off[0], off[1], off[2], off[3]);
		assert_eq!(b.as_array(), expected);
	}

	#[test]
	fn expand_by_noop_on_empty() -> Result<()> {
		let mut b = TileBBox::new_empty(5)?;
		b.expand_by(10, 10, 10, 10);
		assert!(b.is_empty());
		Ok(())
	}

	// ------------------------------ include_bbox ------------------------------
	#[test]
	fn include_bbox_merges_ranges_and_ignores_empty() -> Result<()> {
		let mut a = bb(4, 4, 4, 6, 6);
		let b = bb(4, 2, 5, 8, 7);
		a.include_bbox(&b)?;
		assert_eq!(a.as_array(), [2, 4, 8, 7]);
		let c = TileBBox::new_empty(4)?;
		a.include_bbox(&c)?; // no change
		assert_eq!(a.as_array(), [2, 4, 8, 7]);
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
		a.intersect_with(&b)?;
		if exp == [0, 0, 0, 0] && (a.width() == 0 || a.height() == 0) {
			// empty expected; nothing more to assert
			return Ok(());
		}
		assert_eq!(a.as_array(), exp);
		Ok(())
	}

	#[test]
	fn intersect_with_pyramid_shrinks() -> Result<()> {
		let full = TileBBox::new_full(5)?;
		let pyramid = TileBBoxPyramid::from(&[full]);
		let mut b = bb(5, 12, 12, 20, 20);
		b.intersect_with_pyramid(&pyramid);
		assert_eq!(b, bb(5, 12, 12, 20, 20)); // full pyramid covers all

		let small = bb(5, 14, 15, 16, 18);
		let py_small = TileBBoxPyramid::from(&[small]);
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
		assert_eq!(b.as_array(), [0, 1, 2, 3]);
		b.shift_to(13, 14)?; // move within bounds
		assert_eq!(b.as_array(), [13, 14, 15, 16]);
		Ok(())
	}

	// ------------------------------ scale down / up (and copies) ------------------------------
	#[test]
	fn scale_down_by_powers_of_two() -> Result<()> {
		let mut b = bb(5, 8, 10, 15, 17); // 8x8 region
		b.scale_down(2);
		assert_eq!(b.as_array(), [4, 5, 7, 8]);
		b.scale_down(2);
		assert_eq!(b.as_array(), [2, 2, 3, 4]);
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
		assert_eq!(c.as_array(), [2, 2, 3, 4]);
		// Original unchanged
		assert_eq!(b, bb(5, 8, 10, 15, 17));
		Ok(())
	}

	#[test]
	fn scale_up_and_scaled_up() -> Result<()> {
		let mut b = bb(6, 2, 3, 4, 5); // 3x3
		b.scale_up(2)?;
		assert_eq!(b.as_array(), [4, 6, 9, 11]);
		let c = b.scaled_up(2)?;
		assert_eq!(c.as_array(), [8, 12, 19, 23]);
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
		assert_eq!(b.as_array(), [2, 2, 5, 5]);
		b.level_down(); // back to z=2
		assert_eq!(b.level, 2);
		assert_eq!(b.as_array(), [1, 1, 2, 2]);

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
			let scale = 2u32.pow((target - 8) as u32);
			assert_eq!(b.level, target);
			assert_eq!(b.x_min(), a.x_min() * scale);
			assert_eq!(b.y_min(), a.y_min() * scale);
			assert_eq!(b.x_max(), (a.x_max() + 1) * scale - 1);
			assert_eq!(b.y_max(), (a.y_max() + 1) * scale - 1);
		} else {
			let scale = 2u32.pow((8 - target) as u32);
			assert_eq!(b.level, target);
			assert_eq!(b.x_min(), a.x_min() / scale);
			assert_eq!(b.y_min(), a.y_min() / scale);
			assert_eq!(b.x_max(), a.x_max() / scale);
			assert_eq!(b.y_max(), a.y_max() / scale);
		}
		Ok(())
	}

	// ------------------------------ round / rounded ------------------------------
	#[rstest]
	#[case(1)]
	#[case(2)]
	#[case(4)]
	fn round_to_block_sizes(#[case] block: u32) -> Result<()> {
		let mut b = bb(6, 10, 10, 17, 21); // width=8,height=12
		b.round(block);
		// Edges should align to multiples of block (inclusive max)
		assert_eq!(b.x_min() % block, 0);
		assert_eq!(b.y_min() % block, 0);
		assert_eq!((b.x_max() + 1) % block, 0);
		assert_eq!((b.y_max() + 1) % block, 0);

		let a = bb(6, 3, 5, 3, 5); // 1x1
		let r = a.rounded(block);
		assert_eq!(r.x_min() % block, 0);
		assert_eq!(r.y_min() % block, 0);
		assert_eq!((r.x_max() + 1) % block, 0);
		assert_eq!((r.y_max() + 1) % block, 0);
		Ok(())
	}

	// ------------------------------ flip_y ------------------------------
	#[test]
	fn flip_y_inverts_vertically() -> Result<()> {
		// For z=3, max_coord = 2^3 - 1 = 7
		let mut b = bb(3, 1, 2, 3, 4);
		b.flip_y();
		// y' = max_coord - y_max = 7 - 4 = 3; keep height=3 → y_min'=3, y_max'=5
		assert_eq!(b.as_array(), [1, 3, 3, 5]);
		Ok(())
	}
}
