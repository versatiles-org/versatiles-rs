//! Tile-aligned bounding boxes for a single zoom level.
//!
//! A `TileBBox` describes a **rectangular region of Web‑Mercator tiles** at a
//! specific zoom level `z`. Coordinates are zero-based and inclusive on the
//! maximum side when expressed as `(x_min, y_min, x_max, y_max)`; equivalently
//! the internal representation stores `(x_min, y_min, width, height)` where
//! `width = x_max − x_min + 1` and `height = y_max − y_min + 1`.
//!
//! ## Conventions
//! - Zoom level `z` is in the range `0..=31`.
//! - Tile coordinate range per axis is `0..(2^z − 1)`.
//! - Y increases **downwards** (TMS/XYZ style, north‑up images have negative
//!   pixel height in geotransforms).
//! - An empty bbox has `width == 0` or `height == 0`.
//!
//! ## Common tasks
//! - Build from min+size: [`TileBBox::from_min_and_size`]
//! - Build from min+max:  [`TileBBox::from_min_and_max`]
//! - Cover full level:    [`TileBBox::new_full`]
//! - Empty at level:      [`TileBBox::new_empty`]
//! - Convert from lon/lat: [`TileBBox::from_geo`]
//!
//! ## Examples
//! Create a 3×2 bbox at z=4 starting at (5,6):
//! ```
//! # use versatiles_core::TileBBox;
//! let bb = TileBBox::from_min_and_size(4, 5, 6, 3, 2).unwrap();
//! assert_eq!((bb.x_min(), bb.y_min(), bb.x_max(), bb.y_max()), (5, 6, 7, 7));
//! ```
//! Create the full extent at z=2 and check its size:
//! ```
//! # use versatiles_core::TileBBox;
//! let bb = TileBBox::new_full(2).unwrap();
//! assert_eq!(bb.width(), 4);
//! assert_eq!(bb.height(), 4);
//! ```

use crate::{GeoBBox, TileCoord};
use anyhow::{Result, ensure};
use versatiles_derive::context;

/// A rectangular region of tiles at a specific zoom level.
///
/// The bbox stores the **minimum** tile coordinates and **dimensions**. The
/// derived maximum coordinates are inclusive. A bbox is *empty* when either
/// `width == 0` or `height == 0`.
///
/// # Fields
/// - `level` — zoom level (0..=31).
/// - `x_min`, `y_min` — minimum tile coordinates.
/// - `width`, `height` — dimensions in tiles.
///
/// # Example
/// ```
/// # use versatiles_core::TileBBox;
/// let bb = TileBBox::from_min_and_max(3, 2, 1, 4, 2).unwrap();
/// assert_eq!(bb.width(), 3);  // 4−2+1
/// assert_eq!(bb.height(), 2); // 2−1+1
/// ```

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct TileBBox {
	/// Zoom level of the bounding box.
	pub level: u8,
	/// Minimum x-coordinate.
	x_min: u32,
	/// Minimum y-coordinate.
	y_min: u32,
	/// Width of the bounding box.
	width: u32,
	/// Height of the bounding box.
	height: u32,
}

impl TileBBox {
	/// Create from minimum tile and a size, validating bounds for the given level.
	///
	/// # Errors
	/// Returns an error if any coordinate or extent exceeds the valid range for
	/// the level.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let bb = TileBBox::from_min_and_size(2, 1, 1, 2, 2).unwrap();
	/// assert_eq!((bb.x_min(), bb.y_min(), bb.x_max(), bb.y_max()), (1,1,2,2));
	/// ```
	#[context("Failed to create TileBBox from min ({x_min}, {y_min}) and size ({width}, {height}) at level {level}")]
	pub fn from_min_and_size(level: u8, x_min: u32, y_min: u32, width: u32, height: u32) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");

		let size = 1u32 << level;

		ensure!(x_min < size, "x_min ({x_min}) must be < size ({size})");
		ensure!(y_min < size, "y_min ({y_min}) must be < size ({size})");

		ensure!(
			width + x_min <= size,
			"width ({width}) + x_min ({x_min}) must be <= size ({size})"
		);
		ensure!(
			height + y_min <= size,
			"height ({height}) + y_min ({y_min}) must be <= size ({size})"
		);

		Ok(TileBBox {
			level,
			x_min,
			y_min,
			width,
			height,
		})
	}

	/// Creates a new `TileBBox` with specified coordinates and zoom level.
	///
	/// # Arguments
	///
	/// * `level` - Zoom level of the bounding box (`0..=31`).
	/// * `x_min` - Minimum x-coordinate.
	/// * `y_min` - Minimum y-coordinate.
	/// * `x_max` - Maximum x-coordinate.
	/// * `y_max` - Maximum y-coordinate.
	///
	/// # Returns
	///
	/// * `Ok(TileBBox)` if creation is successful.
	/// * `Err(anyhow::Error)` if any validation fails.
	///
	/// # Errors
	///
	/// - If `level` > 31.
	/// - If any coordinate exceeds the maximum allowed by the zoom level.
	/// - If `x_min > x_max` or `y_min > y_max`.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let bb = TileBBox::from_min_and_max(1, 0, 0, 1, 1).unwrap();
	/// assert_eq!(bb.width(), 2);
	/// assert_eq!(bb.height(), 2);
	/// ```
	#[context("Failed to create TileBBox from min ({x_min}, {y_min}) and max ({x_max}, {y_max}) at level {level}")]
	pub fn from_min_and_max(level: u8, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");

		let max = (1u32 << level) - 1;

		ensure!(x_min <= x_max, "x_min ({x_min}) must be <= x_max ({x_max})");
		ensure!(y_min <= y_max, "y_min ({y_min}) must be <= y_max ({y_max})");
		ensure!(x_max <= max, "x_max ({x_max}) must be <= max ({max})");
		ensure!(y_max <= max, "y_max ({y_max}) must be <= max ({max})");

		Ok(TileBBox {
			level,
			x_min,
			y_min,
			width: x_max + 1 - x_min,
			height: y_max + 1 - y_min,
		})
	}

	/// Creates a `TileBBox` covering the entire range of tiles at the specified zoom level.
	///
	/// # Arguments
	///
	/// * `level` - Zoom level (`0..=31`).
	///
	/// # Returns
	///
	/// * `Ok(TileBBox)` if creation is successful.
	/// * `Err(anyhow::Error)` if the zoom level is invalid.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let bb = TileBBox::new_full(1).unwrap();
	/// assert_eq!((bb.x_min(), bb.y_min(), bb.x_max(), bb.y_max()), (0,0,1,1));
	/// ```
	#[context("Failed to create full TileBBox at level {level}")]
	pub fn new_full(level: u8) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		let max = (1u32 << level) - 1;
		Self::from_min_and_max(level, 0, 0, max, max)
	}

	/// Creates an empty `TileBBox` at the specified zoom level.
	///
	/// An empty bbox has `width == 0` or `height == 0` and thus contains no tiles.
	///
	/// # Arguments
	///
	/// * `level` - Zoom level (`0..=31`).
	///
	/// # Returns
	///
	/// * `Ok(TileBBox)` representing an empty bounding box.
	/// * `Err(anyhow::Error)` if the zoom level is invalid.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::TileBBox;
	/// let bb = TileBBox::new_empty(5).unwrap();
	/// assert!(bb.is_empty());
	/// assert_eq!(bb.width(), 0);
	/// ```
	#[context("Failed to create empty TileBBox at level {level}")]
	pub fn new_empty(level: u8) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		Ok(TileBBox {
			level,
			x_min: 0,
			y_min: 0,
			width: 0,
			height: 0,
		})
	}

	/// Constructs a `TileBBox` from geographical coordinates.
	///
	/// Converts latitude and longitude bounds to tile coordinates based on the zoom level.
	///
	/// Note: conversion uses a half‑open convention on tile edges to ensure
	/// that bboxes aligned to tile boundaries are not expanded spuriously.
	///
	/// # Arguments
	///
	/// * `level` - Zoom level (`0..=31`).
	/// * `bbox` - Geographical bounding box (`GeoBBox`).
	///
	/// # Returns
	///
	/// * `Ok(TileBBox)` if conversion is successful.
	/// * `Err(anyhow::Error)` if any validation fails.
	///
	/// # Errors
	///
	/// - If the geographical coordinates are invalid.
	/// - If the converted tile coordinates are out of bounds.
	///
	/// # Example
	/// ```
	/// # use versatiles_core::{TileBBox, GeoBBox};
	/// let geo = GeoBBox::new(8.0, 51.0, 8.5, 51.3).unwrap();
	/// let bb = TileBBox::from_geo(9, &geo).unwrap();
	/// assert!(!bb.is_empty());
	/// ```
	#[context("Failed to create TileBBox from GeoBBox {bbox:?} at level {level}")]
	pub fn from_geo(level: u8, bbox: &GeoBBox) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");

		// Convert geographical coordinates to tile coordinates
		let p_min = TileCoord::from_geo(bbox.x_min + 1e-10, bbox.y_max - 1e-10, level)?;
		let p_max = TileCoord::from_geo(bbox.x_max - 1e-10, bbox.y_min + 1e-10, level)?;

		Self::from_min_and_max(level, p_min.x, p_min.y, p_max.x, p_max.y)
	}

	/// Calculates the width (in tiles) of the bounding box.
	#[must_use]
	#[inline]
	pub fn width(&self) -> u32 {
		self.width
	}

	/// Calculates the height (in tiles) of the bounding box.
	#[must_use]
	#[inline]
	pub fn height(&self) -> u32 {
		self.height
	}

	/// Minimum x‑tile (column) coordinate.
	#[must_use]
	#[inline]
	pub fn x_min(&self) -> u32 {
		self.x_min
	}

	/// Minimum y‑tile (row) coordinate.
	#[must_use]
	#[inline]
	pub fn y_min(&self) -> u32 {
		self.y_min
	}

	/// Clamp to the level’s maximum if the requested width would exceed bounds.
	pub fn set_width(&mut self, width: u32) {
		self.width = width.min(self.max_count() - self.x_min);
	}

	/// Clamp to the level’s maximum if the requested height would exceed bounds.
	pub fn set_height(&mut self, height: u32) {
		self.height = height.min(self.max_count() - self.y_min);
	}

	/// Sets the minimum x-coordinate, while keeping the maximum x-coordinate consistent.
	#[context("Failed to set x_min to {x_min}")]
	pub fn set_x_min(&mut self, x_min: u32) -> Result<()> {
		ensure!(
			x_min < self.max_count(),
			"x_min ({x_min}) must be < max ({})",
			self.max_count()
		);
		let x_max = self.x_max();
		self.x_min = x_min;
		self.set_x_max(x_max)
	}

	/// Sets the minimum y-coordinate, while keeping the maximum y-coordinate consistent.
	#[context("Failed to set y_min to {y_min}")]
	pub fn set_y_min(&mut self, y_min: u32) -> Result<()> {
		ensure!(
			y_min < self.max_count(),
			"y_min ({y_min}) must be < max ({})",
			self.max_count()
		);
		let y_max = self.y_max();
		self.y_min = y_min;
		self.set_y_max(y_max)
	}

	/// Returns the maximum x-coordinate of the bounding box.
	#[must_use]
	pub fn x_max(&self) -> u32 {
		(self.x_min + self.width).saturating_sub(1)
	}

	/// Returns the maximum y-coordinate of the bounding box.
	#[must_use]
	pub fn y_max(&self) -> u32 {
		(self.y_min + self.height).saturating_sub(1)
	}

	/// Sets the maximum x-coordinate, while keeping the minimum x-coordinate consistent.
	#[context("Failed to set x_max to {x_max}")]
	pub fn set_x_max(&mut self, x_max: u32) -> Result<()> {
		ensure!(
			x_max < self.max_count(),
			"x_max ({x_max}) must be < max ({})",
			self.max_count()
		);
		if x_max >= self.x_min {
			self.width = x_max - self.x_min + 1;
		} else {
			self.width = 0;
		}
		Ok(())
	}

	/// Sets the maximum y-coordinate, while keeping the minimum y-coordinate consistent.
	#[context("Failed to set y_max to {y_max}")]
	pub fn set_y_max(&mut self, y_max: u32) -> Result<()> {
		ensure!(
			y_max < self.max_count(),
			"y_max ({y_max}) must be < max ({})",
			self.max_count()
		);
		if y_max >= self.y_min {
			self.height = y_max - self.y_min + 1;
		} else {
			self.height = 0;
		}
		Ok(())
	}

	/// Swap X and Y axes for a non‑empty bbox (coordinates and dimensions).
	pub fn swap_xy(&mut self) {
		if !self.is_empty() {
			std::mem::swap(&mut self.x_min, &mut self.y_min);
			std::mem::swap(&mut self.width, &mut self.height);
		}
	}
	/// Sets the bounding box to an empty state.
	///
	/// After calling this method, `is_empty()` will return `true`.
	pub fn set_empty(&mut self) {
		self.width = 0;
		self.height = 0;
	}

	/// Sets the bbox to cover all tiles at its level (convenience for tests).
	pub fn set_full(&mut self) {
		let max = self.max_count();
		self.x_min = 0;
		self.y_min = 0;
		self.width = max;
		self.height = max;
	}

	/// Sets the bounding box to a full state, covering the entire tile range at its zoom level.
	///
	/// # Panics
	///
	/// Panics if the zoom level is invalid.
	///
	/// # Note
	///
	/// This method is primarily used for testing purposes.
	#[context("Failed to set TileBBox to {x_min}, {y_min}, {width}, {height}")]
	pub fn set_min_and_size(&mut self, x_min: u32, y_min: u32, width: u32, height: u32) -> Result<()> {
		let max = self.max_count();
		ensure!(x_min < max, "x_min ({x_min}) must be < max ({max})");
		ensure!(y_min < max, "y_min ({y_min}) must be < max ({max})");
		ensure!(
			x_min + width <= max,
			"x_min + width ({}) must be <= max ({max})",
			x_min + width
		);
		ensure!(
			y_min + height <= max,
			"y_min + height ({}) must be <= max ({max})",
			y_min + height
		);
		self.x_min = x_min;
		self.y_min = y_min;
		self.width = width;
		self.height = height;
		Ok(())
	}

	#[context("Failed to set TileBBox to min ({x_min}, {y_min}) and max ({x_max}, {y_max})")]
	pub fn set_min_and_max(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> Result<()> {
		ensure!(x_min <= x_max, "x_min ({x_min}) must be <= x_max ({x_max})");
		ensure!(y_min <= y_max, "y_min ({y_min}) must be <= y_max ({y_max})");

		let max = self.max_count();

		ensure!(x_max < max, "x_max ({x_max}) must be < max ({max})");
		ensure!(y_max < max, "y_max ({y_max}) must be < max ({max})");

		self.x_min = x_min;
		self.y_min = y_min;
		self.width = x_max - x_min + 1;
		self.height = y_max - y_min + 1;
		Ok(())
	}
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[rstest]
	#[case((4, 5, 12, 5, 12), 1)]
	#[case((4, 5, 12, 7, 15), 12)]
	#[case((4, 5, 12, 5, 15), 4)]
	#[case((4, 5, 15, 7, 15), 3)]
	fn count_tiles_cases(#[case] args: (u8, u32, u32, u32, u32), #[case] expected: u64) {
		let (l, x0, y0, x1, y1) = args;
		assert_eq!(
			TileBBox::from_min_and_max(l, x0, y0, x1, y1).unwrap().count_tiles(),
			expected
		);
	}

	#[test]
	fn from_geo() {
		let bbox1 = TileBBox::from_geo(9, &GeoBBox::new(8.0653, 51.3563, 12.3528, 52.2564).unwrap()).unwrap();
		let bbox2 = TileBBox::from_min_and_max(9, 267, 168, 273, 170).unwrap();
		assert_eq!(bbox1, bbox2);
	}

	#[test]
	fn from_geo_is_not_empty() {
		let bbox1 = TileBBox::from_geo(0, &GeoBBox::new(8.0, 51.0, 8.000001f64, 51.0).unwrap()).unwrap();
		assert_eq!(bbox1.count_tiles(), 1);
		assert!(!bbox1.is_empty());

		let bbox2 = TileBBox::from_geo(14, &GeoBBox::new(-132.000001, -40.0, -132.0, -40.0).unwrap()).unwrap();
		assert_eq!(bbox2.count_tiles(), 1);
		assert!(!bbox2.is_empty());
	}

	use anyhow::Result;

	// ------------------------------
	// from_min_and_size
	// ------------------------------
	#[rstest]
	#[case((1, 0, 0, 1, 1))]
	#[case((2, 0, 0, 2, 2))]
	#[case((4, 5, 6, 3, 2))]
	fn from_min_and_size_valid(#[case] args: (u8, u32, u32, u32, u32)) -> Result<()> {
		let (lvl, x0, y0, w, h) = args;
		let bb = TileBBox::from_min_and_size(lvl, x0, y0, w, h)?;
		assert_eq!(bb.level, lvl);
		assert_eq!(bb.x_min(), x0);
		assert_eq!(bb.y_min(), y0);
		assert_eq!(bb.width(), w);
		assert_eq!(bb.height(), h);
		assert_eq!(bb.x_max(), x0 + w - 1);
		assert_eq!(bb.y_max(), y0 + h - 1);
		Ok(())
	}

	#[rstest]
	#[case((32, 0, 0, 1, 1))] // invalid level
	#[case((3, 8, 0, 1, 1))] // x_min > max
	#[case((3, 0, 8, 1, 1))] // y_min > max
	#[case((2, 2, 2, 3, 2))] // x_max > max
	#[case((2, 0, 2, 2, 3))] // y_max > max
	fn from_min_and_size_invalid(#[case] args: (u8, u32, u32, u32, u32)) {
		let (lvl, x0, y0, w, h) = args;
		assert!(TileBBox::from_min_and_size(lvl, x0, y0, w, h).is_err());
	}

	// ------------------------------
	// from_min_and_max
	// ------------------------------
	#[rstest]
	#[case((0, 0, 0, 0, 0))]
	#[case((4, 5, 6, 7, 9))]
	fn from_min_and_max_valid(#[case] args: (u8, u32, u32, u32, u32)) -> Result<()> {
		let (lvl, x0, y0, x1, y1) = args;
		let bb = TileBBox::from_min_and_max(lvl, x0, y0, x1, y1)?;
		assert_eq!(bb.level, lvl);
		assert_eq!(bb.x_min(), x0);
		assert_eq!(bb.y_min(), y0);
		assert_eq!(bb.x_max(), x1);
		assert_eq!(bb.y_max(), y1);
		assert_eq!(bb.width(), x1 - x0 + 1);
		assert_eq!(bb.height(), y1 - y0 + 1);
		Ok(())
	}

	#[rstest]
	#[case((32, 0, 0, 0, 0))] // invalid level
	#[case((3, 5, 6, 4, 6))] // x_min > x_max
	#[case((3, 5, 6, 5, 5))] // y_min > y_max
	#[case((2, 0, 0, 5, 0))] // x_max > max
	#[case((2, 0, 0, 0, 5))] // y_max > max
	fn from_min_and_max_invalid(#[case] args: (u8, u32, u32, u32, u32)) {
		let (lvl, x0, y0, x1, y1) = args;
		assert!(TileBBox::from_min_and_max(lvl, x0, y0, x1, y1).is_err());
	}

	// ------------------------------
	// new_full / new_empty
	// ------------------------------
	#[rstest]
	#[case(0)]
	#[case(5)]
	#[case(10)]
	fn new_full_covers_all(#[case] lvl: u8) -> Result<()> {
		let bb = TileBBox::new_full(lvl)?;
		let max = (1u32 << lvl) - 1;
		assert_eq!(bb.x_min(), 0);
		assert_eq!(bb.y_min(), 0);
		assert_eq!(bb.x_max(), max);
		assert_eq!(bb.y_max(), max);
		assert_eq!(bb.width(), max + 1);
		assert_eq!(bb.height(), max + 1);
		Ok(())
	}

	#[rstest]
	#[case(0)]
	#[case(8)]
	fn new_empty_is_empty(#[case] lvl: u8) -> Result<()> {
		let bb = TileBBox::new_empty(lvl)?;
		assert_eq!(bb.width(), 0);
		assert_eq!(bb.height(), 0);
		assert!(bb.is_empty());
		Ok(())
	}

	// ------------------------------
	// setters & clamping
	// ------------------------------
	#[test]
	fn set_width_height_clamp_to_bounds() -> Result<()> {
		let lvl = 3u8; // max coord = 7, count = 8
		let mut bb = TileBBox::from_min_and_max(lvl, 6, 6, 7, 7)?; // 2x2 at bottom-right
		assert_eq!(bb.width(), 2);
		assert_eq!(bb.height(), 2);

		// Expanding width/height should clamp at image bounds
		bb.set_width(10);
		bb.set_height(10);
		assert_eq!(bb.x_min(), 6);
		assert_eq!(bb.y_min(), 6);
		assert_eq!(bb.x_max(), 7);
		assert_eq!(bb.y_max(), 7);
		assert_eq!(bb.width(), 2);
		assert_eq!(bb.height(), 2);
		Ok(())
	}

	#[test]
	fn set_min_max_adjusts_size_and_handles_empty() -> Result<()> {
		let lvl = 4u8;
		let mut bb = TileBBox::from_min_and_max(lvl, 3, 3, 5, 5)?;
		assert_eq!(bb.width(), 3);
		assert_eq!(bb.height(), 3);

		// Move min forward, keep previous max
		bb.set_x_min(4)?;
		bb.set_y_min(4)?;
		assert_eq!(bb.x_min(), 4);
		assert_eq!(bb.y_min(), 4);
		assert_eq!(bb.x_max(), 5);
		assert_eq!(bb.y_max(), 5);
		assert_eq!(bb.width(), 2);
		assert_eq!(bb.height(), 2);

		// Shrink to empty by setting max < min
		bb.set_x_max(3)?; // x_max < x_min → empty in x dimension
		assert_eq!(bb.width(), 0);
		bb.set_y_max(3)?; // y_max < y_min → empty in y dimension
		assert_eq!(bb.height(), 0);
		assert!(bb.is_empty());
		Ok(())
	}

	#[test]
	fn set_min_max_bounds_errors() -> Result<()> {
		let lvl = 2u8; // max = 3
		let mut bb = TileBBox::from_min_and_max(lvl, 1, 1, 2, 2)?;
		// x_max = 3 is ok; 4 is not
		assert!(bb.set_x_max(4).is_err());
		// y_max = 3 is ok; 5 is not
		assert!(bb.set_y_max(5).is_err());
		// x_min < max_count required
		assert!(bb.set_x_min(4).is_err());
		// y_min < max_count required
		assert!(bb.set_y_min(4).is_err());
		Ok(())
	}

	// ------------------------------
	// swap_xy / set_empty / set_full / set_min_and_*
	// ------------------------------
	#[test]
	fn swap_and_empty_full_and_setters() -> Result<()> {
		let lvl = 3u8;
		let mut bb = TileBBox::from_min_and_max(lvl, 1, 2, 3, 5)?;
		assert_eq!(bb.width(), 3);
		assert_eq!(bb.height(), 4);

		// swap xy
		bb.swap_xy();
		assert_eq!(bb.x_min(), 2);
		assert_eq!(bb.y_min(), 1);
		assert_eq!(bb.width(), 4);
		assert_eq!(bb.height(), 3);

		// set_empty
		bb.set_empty();
		assert!(bb.is_empty());
		assert_eq!(bb.width(), 0);
		assert_eq!(bb.height(), 0);

		// set_full
		bb.set_full();
		let max = (1u32 << lvl) - 1;
		assert_eq!(bb.x_min(), 0);
		assert_eq!(bb.y_min(), 0);
		assert_eq!(bb.x_max(), max);
		assert_eq!(bb.y_max(), max);

		// set_min_and_size
		bb.set_min_and_size(1, 1, 2, 2)?;
		assert_eq!(bb.x_min(), 1);
		assert_eq!(bb.y_min(), 1);
		assert_eq!(bb.width(), 2);
		assert_eq!(bb.height(), 2);

		// set_min_and_max
		bb.set_min_and_max(2, 2, 3, 3)?;
		assert_eq!(bb.x_min(), 2);
		assert_eq!(bb.y_min(), 2);
		assert_eq!(bb.x_max(), 3);
		assert_eq!(bb.y_max(), 3);
		assert_eq!(bb.width(), 2);
		assert_eq!(bb.height(), 2);
		Ok(())
	}
}
