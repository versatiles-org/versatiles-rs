//! This module defines the `TileBBox` struct, representing a bounding box for tiles at a specific zoom level.
//! It provides methods to create, manipulate, and query these bounding boxes.
//!
//! # Overview
//!
//! The `TileBBox` struct is used to define a rectangular area of tiles within a specific zoom level.
//! It supports operations such as inclusion, intersection, scaling, and iteration over tile coordinates.
//! This is particularly useful in mapping applications where tile management is essential.

use super::{GeoBBox, TileBBoxPyramid, TileCoord};
use anyhow::{Result, ensure};
use itertools::Itertools;
use std::{
	fmt,
	ops::{Div, Rem},
};

/// Represents a bounding box for tiles at a specific zoom level.
///
/// The bounding box is defined by its minimum and maximum x and y coordinates,
/// along with the zoom level. It can represent an empty area if the maximum
/// coordinates are less than the minimum coordinates.
///
/// # Fields
///
/// - `level`: Zoom level (0..=31).
/// - `x_min`, `y_min`: Minimum tile coordinates.
/// - `x_max`, `y_max`: Maximum tile coordinates.
/// - `max`: Largest valid coordinate at the given zoom level (`2^level - 1`).
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
	// -------------------------------------------------------------------------
	// Constructors
	// -------------------------------------------------------------------------

	pub fn from_min_wh(level: u8, x_min: u32, y_min: u32, width: u32, height: u32) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");

		let max = (1u32 << level) - 1;

		ensure!(x_min <= max, "x_min ({x_min}) must be <= max ({max})");
		ensure!(y_min <= max, "y_min ({y_min}) must be <= max ({max})");
		let x_max = x_min + width - 1;
		ensure!(x_max <= max, "x_max ({x_max}) must be <= max ({max})");
		let y_max = y_min + height - 1;
		ensure!(y_max <= max, "y_max ({y_max}) must be <= max ({max})");

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
	pub fn from_min_max(level: u8, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> Result<TileBBox> {
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
			width: x_max - x_min + 1,
			height: y_max - y_min + 1,
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
	pub fn new_full(level: u8) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		let max = 1u32 << level;
		Self::from_min_wh(level, 0, 0, max, max)
	}

	/// Creates an empty `TileBBox` at the specified zoom level.
	///
	/// An empty bounding box is defined by `x_max < x_min` or `y_max < y_min`.
	///
	/// # Arguments
	///
	/// * `level` - Zoom level (`0..=31`).
	///
	/// # Returns
	///
	/// * `Ok(TileBBox)` representing an empty bounding box.
	/// * `Err(anyhow::Error)` if the zoom level is invalid.
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
	pub fn from_geo(level: u8, bbox: &GeoBBox) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		bbox.check()?; // Validate GeoBBox

		// Convert geographical coordinates to tile coordinates
		let p_min = TileCoord::from_geo(bbox.0 + 1e-10, bbox.3 - 1e-10, level)?;
		let p_max = TileCoord::from_geo(bbox.2 - 1e-10, bbox.1 + 1e-10, level)?;

		Self::from_min_max(level, p_min.x, p_min.y, p_max.x, p_max.y)
	}

	// -------------------------------------------------------------------------
	// Basic Queries
	// -------------------------------------------------------------------------

	/// Determines if the bounding box is empty.
	///
	/// # Returns
	///
	/// * `true` if `x_max < x_min` or `y_max < y_min`.
	/// * `false` otherwise.
	pub fn is_empty(&self) -> bool {
		self.width == 0 || self.height == 0
	}

	/// Calculates the width (in tiles) of the bounding box.
	///
	/// # Returns
	///
	/// * Width as `u32`.
	/// * `0` if the bounding box is empty.
	pub fn width(&self) -> u32 {
		self.width
	}

	/// Calculates the height (in tiles) of the bounding box.
	///
	/// # Returns
	///
	/// * Height as `u32`.
	/// * `0` if the bounding box is empty.
	pub fn height(&self) -> u32 {
		self.height
	}

	/// Sets the width (in tiles) of the bounding box.
	pub fn set_width(&mut self, width: u32) {
		self.width = width.min(self.max_count() - self.x_min);
	}

	/// Sets the height (in tiles) of the bounding box.
	pub fn set_height(&mut self, height: u32) {
		self.height = height.min(self.max_count() - self.y_min);
	}

	/// Returns the minimum x-coordinate of the bounding box.
	pub fn x_min(&self) -> u32 {
		self.x_min
	}

	/// Returns the minimum y-coordinate of the bounding box.
	pub fn y_min(&self) -> u32 {
		self.y_min
	}

	/// Sets the minimum x-coordinate, while keeping the maximum x-coordinate consistent.
	pub fn set_x_min(&mut self, x_min: u32) {
		let x_max = self.x_max();
		self.x_min = x_min;
		self.set_x_max(x_max);
	}

	/// Sets the minimum y-coordinate, while keeping the maximum y-coordinate consistent.
	pub fn set_y_min(&mut self, y_min: u32) {
		let y_max = self.y_max();
		self.y_min = y_min;
		self.set_y_max(y_max);
	}

	/// Returns the maximum x-coordinate of the bounding box.
	pub fn x_max(&self) -> u32 {
		(self.x_min + self.width).saturating_sub(1)
	}

	/// Returns the maximum y-coordinate of the bounding box.
	pub fn y_max(&self) -> u32 {
		(self.y_min + self.height).saturating_sub(1)
	}

	/// Sets the maximum x-coordinate, while keeping the minimum x-coordinate consistent.
	pub fn set_x_max(&mut self, x_max: u32) {
		if x_max >= self.x_min {
			self.width = x_max.min(self.max_count() - 1) - self.x_min + 1;
		} else {
			self.width = 0;
		}
	}

	/// Sets the maximum y-coordinate, while keeping the minimum y-coordinate consistent.
	pub fn set_y_max(&mut self, y_max: u32) {
		if y_max >= self.y_min {
			self.height = y_max.min(self.max_count() - 1) - self.y_min + 1;
		} else {
			self.height = 0;
		}
	}

	/// Counts the total number of tiles within the bounding box.
	///
	/// # Returns
	///
	/// * Number of tiles as `u64`.
	/// * `0` if the bounding box is empty.
	pub fn count_tiles(&self) -> u64 {
		(self.width as u64) * (self.height as u64)
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
	pub fn is_full(&self) -> bool {
		let max = self.max_count();
		self.x_min == 0 && self.y_min == 0 && self.width == max && self.height == max
	}

	fn max_count(&self) -> u32 {
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
	pub fn contains(&self, coord: &TileCoord) -> bool {
		coord.level == self.level
			&& coord.x >= self.x_min
			&& coord.x < self.x_min + self.width
			&& coord.y >= self.y_min
			&& coord.y < self.y_min + self.height
	}

	// -------------------------------------------------------------------------
	// Mutation Methods
	// -------------------------------------------------------------------------

	/// Sets the bounding box to an empty state.
	///
	/// After calling this method, `is_empty()` will return `true`.
	pub fn set_empty(&mut self) {
		self.width = 0;
		self.height = 0;
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
	#[cfg(test)]
	pub fn set_full(&mut self) {
		let max = self.max_count();
		self.x_min = 0;
		self.y_min = 0;
		self.width = max;
		self.height = max;
	}

	/// Includes a specific tile coordinate (`x`, `y`) within the bounding box.
	///
	/// Expands the bounding box to encompass the given coordinate. If the bounding box is empty,
	/// it will be set to the provided coordinate.
	///
	/// # Arguments
	///
	/// * `x` - X-coordinate of the tile.
	/// * `y` - Y-coordinate of the tile.
	///
	/// # Panics
	///
	/// Panics if the resulting bounding box is invalid.
	pub fn include(&mut self, x: u32, y: u32) {
		if self.is_empty() {
			// Initialize bounding box to the provided coordinate
			self.x_min = x;
			self.y_min = y;
			self.width = 1;
			self.height = 1;
		} else {
			// Expand bounding box to include the new coordinate
			if x < self.x_min {
				self.set_x_min(x);
			} else if x > self.x_max() {
				self.set_x_max(x);
			}
			if y < self.y_min {
				self.set_y_min(y);
			} else if y > self.y_max() {
				self.set_y_max(y);
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
	pub fn include_coord(&mut self, coord: &TileCoord) -> Result<()> {
		if coord.level != self.level {
			return Err(anyhow::anyhow!(
				"Cannot include TileCoord with z={} into TileBBox at z={}",
				coord.level,
				self.level
			));
		}
		self.include(coord.x, coord.y);
		Ok(())
	}

	/// Adds a border to the bounding box.
	///
	/// Expands the bounding box by subtracting `x_min` and `y_min` from the minimum coordinates
	/// and adding `x_max` and `y_max` to the maximum coordinates. The expansion is clamped
	/// to the valid range defined by the zoom level.
	///
	/// # Arguments
	///
	/// * `x_min` - Amount to subtract from `x_min`.
	/// * `y_min` - Amount to subtract from `y_min`.
	/// * `x_max` - Amount to add to `x_max`.
	/// * `y_max` - Amount to add to `y_max`.
	///
	/// # Returns
	///
	/// * `Ok(())` if the border is added successfully.
	/// * `Err(anyhow::Error)` if the resulting bounding box is invalid.
	pub fn expand_by(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) {
		if !self.is_empty() {
			let x_max = self.x_max().saturating_add(x_max);
			let y_max = self.y_max().saturating_add(y_max);
			self.x_min = self.x_min.saturating_sub(x_min);
			self.y_min = self.y_min.saturating_sub(y_min);
			self.set_x_max(x_max);
			self.set_y_max(y_max);
		}
	}

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

		Ok(self.x_min <= bbox.x_min
			&& self.x_max() >= bbox.x_max()
			&& self.y_min <= bbox.y_min
			&& self.y_max() >= bbox.y_max())
	}

	// -------------------------------------------------------------------------
	// Include and Intersect Operations
	// -------------------------------------------------------------------------

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
			let x_max = self.x_max().max(bbox.x_max());
			let y_max = self.y_max().max(bbox.y_max());
			self.x_min = self.x_min.min(bbox.x_min);
			self.y_min = self.y_min.min(bbox.y_min);
			self.set_x_max(x_max);
			self.set_y_max(y_max);
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

		let x_max = self.x_max().min(bbox.x_max());
		let y_max = self.y_max().min(bbox.y_max());
		self.x_min = self.x_min.max(bbox.x_min);
		self.y_min = self.y_min.max(bbox.y_min);
		self.set_x_max(x_max);
		self.set_y_max(y_max);

		Ok(())
	}

	/// Intersects the bounding box with a `TileBBoxPyramid`.
	///
	/// Modifies this bounding box to represent the overlapping area with the pyramid's bounding box
	/// at the current zoom level.
	///
	/// # Arguments
	///
	/// * `pyramid` - Reference to the `TileBBoxPyramid`.
	pub fn intersect_with_pyramid(&mut self, pyramid: &TileBBoxPyramid) {
		self.intersect_with(pyramid.get_level_bbox(self.level)).unwrap()
	}

	/// Determines if this bounding box overlaps with another bounding box.
	///
	/// Two bounding boxes overlap if their x and y ranges intersect.
	///
	/// # Arguments
	///
	/// * `bbox` - Reference to the `TileBBox` to check overlap with.
	///
	/// # Returns
	///
	/// * `true` if the bounding boxes overlap.
	/// * `false` otherwise.
	///
	/// # Errors
	///
	/// * Returns an error if the zoom levels do not match.
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

		Ok(self.x_min <= bbox.x_max()
			&& self.x_max() >= bbox.x_min
			&& self.y_min <= bbox.y_max()
			&& self.y_max() >= bbox.y_min)
	}

	// -------------------------------------------------------------------------
	// Coordinate Transformations
	// -------------------------------------------------------------------------

	/// Converts the bounding box to geographical coordinates (`GeoBBox`).
	///
	/// # Returns
	///
	/// * `GeoBBox` representing the geographical area covered by this bounding box.
	pub fn to_geo_bbox(&self) -> GeoBBox {
		// Bottom-left in geospatial terms is (x_min, y_max + 1)
		let p_min = TileCoord::new(self.level, self.x_min, self.y_max() + 1)
			.unwrap()
			.as_geo();
		// Top-right in geospatial terms is (x_max + 1, y_min)
		let p_max = TileCoord::new(self.level, self.x_max() + 1, self.y_min)
			.unwrap()
			.as_geo();

		GeoBBox(p_min[0], p_min[1], p_max[0], p_max[1])
	}

	/// Shifts the bounding box by specified amounts in the x and y directions.
	/// # Arguments
	///
	/// * `x` - Amount to shift in the x-direction.
	/// * `y` - Amount to shift in the y-direction.
	///
	/// # Returns
	///
	/// * `Ok(())` if the shift is successful.
	/// * `Err(anyhow::Error)` if the resulting bounding box is invalid.
	pub fn shift_by(&mut self, x: i64, y: i64) {
		self.shift_to(
			(self.x_min as i64 + x).max(0) as u32,
			(self.y_min as i64 + y).max(0) as u32,
		);
	}

	pub fn shift_to(&mut self, x_min: u32, y_min: u32) {
		self.x_min = x_min;
		self.y_min = y_min;
		let max = self.max_count() - 1;
		if self.x_max() > max {
			self.set_x_max(max);
		}
		if self.y_max() > max {
			self.set_y_max(max);
		}
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

		let x_max = self.x_max() / scale;
		let y_max = self.y_max() / scale;
		self.x_min /= scale;
		self.y_min /= scale;
		self.set_x_max(x_max);
		self.set_y_max(y_max);
	}

	pub fn scaled_down(&self, scale: u32) -> TileBBox {
		let mut bbox = self.clone();
		bbox.scale_down(scale);
		bbox
	}

	pub fn scale_up(&mut self, scale: u32) {
		assert!(scale > 0, "scale must be greater than 0");

		let x_max = (self.x_max() + 1) * scale - 1;
		let y_max = (self.y_max() + 1) * scale - 1;
		self.x_min *= scale;
		self.y_min *= scale;
		self.set_x_max(x_max);
		self.set_y_max(y_max);
	}

	pub fn scaled_up(&self, scale: u32) -> TileBBox {
		let mut bbox = self.clone();
		bbox.scale_up(scale);
		bbox
	}

	pub fn level_up(&mut self) {
		assert!(self.level < 31, "level must be less than 31");
		self.level += 1;
		self.scale_up(2);
	}

	pub fn level_down(&mut self) {
		assert!(self.level > 0, "level must be greater than 0");
		self.level -= 1;
		self.scale_down(2);
	}

	pub fn leveled_up(&self) -> TileBBox {
		let mut c = *self;
		c.level_up();
		c
	}

	pub fn leveled_down(&self) -> TileBBox {
		let mut c = *self;
		c.level_down();
		c
	}

	pub fn at_level(&self, level: u8) -> TileBBox {
		assert!(level <= 31, "level ({level}) must be <= 31");

		let mut bbox = if level > self.level {
			let scale = 2u32.pow((level - self.level) as u32);
			self.scaled_up(scale)
		} else {
			let scale = 2u32.pow((self.level - level) as u32);
			self.scaled_down(scale)
		};
		bbox.level = level;
		bbox
	}

	pub fn min_corner(&self) -> TileCoord {
		TileCoord::new(self.level, self.x_min, self.y_min).unwrap()
	}

	pub fn max_corner(&self) -> TileCoord {
		TileCoord::new(self.level, self.x_max(), self.y_max()).unwrap()
	}

	pub fn dimensions(&self) -> (u32, u32) {
		(self.width(), self.height())
	}

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

		let x = self.x_min;
		let y = self.y_min;
		let w = self.width() / 2;
		let h = self.height() / 2;

		let bbox = match quadrant {
			0 => TileBBox::from_min_wh(self.level, x, y, w, h)?,     // Top-left
			1 => TileBBox::from_min_wh(self.level, x + w, y, w, h)?, // Top-right
			2 => TileBBox::from_min_wh(self.level, x, y + h, w, h)?, // Bottom-left
			3 => TileBBox::from_min_wh(self.level, x + w, y + h, w, h)?, // Bottom-right
			_ => unreachable!(),
		};

		Ok(bbox)
	}

	// -------------------------------------------------------------------------
	// Iteration Methods
	// -------------------------------------------------------------------------

	/// Returns an iterator over all tile coordinates within the bounding box.
	///
	/// The iteration is in row-major order.
	///
	/// # Returns
	///
	/// An iterator yielding `TileCoord` instances.
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
		let y_range = self.y_min..=self.y_max();
		let x_range = self.x_min..=self.x_max();
		y_range
			.cartesian_product(x_range)
			.map(|(y, x)| TileCoord::new(self.level, x, y).unwrap())
	}

	/// Consumes the bounding box and returns an iterator over all tile coordinates within it.
	///
	/// The iteration is in row-major order.
	///
	/// # Returns
	///
	/// An iterator yielding `TileCoord` instances.
	pub fn into_iter_coords(self) -> impl Iterator<Item = TileCoord> {
		let y_range = self.y_min..=self.y_max();
		let x_range = self.x_min..=self.x_max();
		y_range
			.cartesian_product(x_range)
			.map(move |(y, x)| TileCoord::new(self.level, x, y).unwrap())
	}

	/// Splits the bounding box into a grid of smaller bounding boxes of a specified size.
	///
	/// Each sub-bounding box will have dimensions at most `size x size` tiles.
	/// The last sub-bounding boxes in each row or column may be smaller if the original
	/// dimensions are not exact multiples of `size`.
	///
	/// # Arguments
	///
	/// * `size` - Maximum size of each grid cell.
	///
	/// # Returns
	///
	/// An iterator yielding `TileBBox` instances representing the grid.
	pub fn iter_bbox_grid(&self, size: u32) -> Box<dyn Iterator<Item = TileBBox> + '_> {
		assert!(size != 0, "size must be greater than 0");

		let level = self.level;
		let max = (1u32 << level) - 1;
		let mut meta_bbox = *self;
		meta_bbox.scale_down(size);

		let iter = meta_bbox
			.iter_coords()
			.map(move |coord| {
				let x = coord.x * size;
				let y = coord.y * size;

				let mut bbox =
					TileBBox::from_min_max(level, x, y, (x + size - 1).min(max), (y + size - 1).min(max)).unwrap();
				bbox.intersect_with(self).unwrap();
				bbox
			})
			.filter(|bbox| !bbox.is_empty())
			.collect::<Vec<TileBBox>>()
			.into_iter();

		Box::new(iter)
	}

	/// Retrieves the 0-based index of a `TileCoord` within the bounding box.
	///
	/// # Arguments
	///
	/// * `coord` - Reference to the tile coordinate.
	///
	/// # Returns
	///
	/// * `Ok(usize)` representing the index if the coordinate is within the bounding box.
	/// * `Err(anyhow::Error)` if the coordinate is outside the bounding box or zoom levels do not match.
	pub fn index_of(&self, coord: &TileCoord) -> Result<u64> {
		ensure!(
			self.contains(coord),
			"Coordinate {coord:?} is not within the bounding box {self:?}",
		);

		let x = (coord.x - self.x_min) as u64;
		let y = (coord.y - self.y_min) as u64;
		let index = y * (self.width as u64) + x;

		Ok(index)
	}

	/// Retrieves the `TileCoord` at a specific index within the bounding box.
	///
	/// # Arguments
	///
	/// * `index` - 0-based index of the tile coordinate.
	///
	/// # Returns
	///
	/// * `Ok(TileCoord)` if the index is within bounds.
	/// * `Err(anyhow::Error)` if the index is out of bounds.
	pub fn coord_at_index(&self, index: u64) -> Result<TileCoord> {
		ensure!(index < self.count_tiles(), "index {index} out of bounds");

		let width = self.width() as u64;
		let x = index.rem(width) as u32 + self.x_min;
		let y = index.div(width) as u32 + self.y_min;
		TileCoord::new(self.level, x, y)
	}

	pub fn round(&mut self, block_size: u32) {
		let x_max = (self.x_max() + 1).div_ceil(block_size) * block_size - 1;
		let y_max = (self.y_max() + 1).div_ceil(block_size) * block_size - 1;
		self.x_min = (self.x_min / block_size) * block_size;
		self.y_min = (self.y_min / block_size) * block_size;
		self.set_x_max(x_max);
		self.set_y_max(y_max);
	}

	pub fn rounded(&self, block_size: u32) -> TileBBox {
		let mut bbox = *self;
		bbox.round(block_size);
		bbox
	}

	pub fn max_coord_at_level(&self) -> u32 {
		(1u32 << self.level) - 1
	}

	pub fn as_string(&self) -> String {
		format!(
			"{}:[{},{},{},{}]",
			self.level,
			self.x_min,
			self.y_min,
			self.x_max(),
			self.y_max()
		)
	}

	pub fn flip_y(&mut self) {
		if !self.is_empty() {
			self.shift_to(self.x_min(), self.max_coord_at_level() - self.y_max());
		}
	}
	pub fn swap_xy(&mut self) {
		if !self.is_empty() {
			std::mem::swap(&mut self.x_min, &mut self.y_min);
			std::mem::swap(&mut self.width, &mut self.height);
		}
	}
}

// ----------------------------------------------------------------------------
// Trait Implementations
// ----------------------------------------------------------------------------

impl fmt::Debug for TileBBox {
	/// Formats the bounding box as:
	/// `level: [x_min,y_min,x_max,y_max] (count_tiles)`
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{}: [{},{},{},{}] ({}x{})",
			self.level,
			self.x_min,
			self.y_min,
			self.x_max(),
			self.y_max(),
			self.width(),
			self.height()
		)
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
			TileBBox::from_min_max(l, x0, y0, x1, y1).unwrap().count_tiles(),
			expected
		);
	}

	#[test]
	fn from_geo() {
		let bbox1 = TileBBox::from_geo(9, &GeoBBox(8.0653, 51.3563, 12.3528, 52.2564)).unwrap();
		let bbox2 = TileBBox::from_min_max(9, 267, 168, 273, 170).unwrap();
		assert_eq!(bbox1, bbox2);
	}

	#[test]
	fn from_geo_is_not_empty() {
		let bbox1 = TileBBox::from_geo(0, &GeoBBox(8.0, 51.0, 8.000001f64, 51.0)).unwrap();
		assert_eq!(bbox1.count_tiles(), 1);
		assert!(!bbox1.is_empty());

		let bbox2 = TileBBox::from_geo(14, &GeoBBox(-132.000001, -40.0, -132.0, -40.0)).unwrap();
		assert_eq!(bbox2.count_tiles(), 1);
		assert!(!bbox2.is_empty());
	}

	#[test]
	fn quarter_planet() {
		let geo_bbox = GeoBBox(0.0, -85.05112877980659f64, 180.0, 0.0);
		for level in 1..32 {
			let bbox = TileBBox::from_geo(level, &geo_bbox).unwrap();
			assert_eq!(bbox.count_tiles(), 4u64.pow(level as u32 - 1));
			assert_eq!(bbox.to_geo_bbox(), geo_bbox);
		}
	}

	#[test]
	fn sa_pacific() {
		let geo_bbox = GeoBBox(-180.0, -66.51326044311186f64, -90.0, 0.0);
		for level in 2..32 {
			let bbox = TileBBox::from_geo(level, &geo_bbox).unwrap();
			assert_eq!(bbox.count_tiles(), 4u64.pow(level as u32 - 2));
			assert_eq!(bbox.to_geo_bbox(), geo_bbox);
		}
	}

	#[test]
	fn boolean_operations() {
		/*
			  #---#
		  #---# |
		  | | | |
		  | #-|-#
		  #---#
		*/
		let bbox1 = TileBBox::from_min_max(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::from_min_max(4, 1, 10, 3, 12).unwrap();

		let mut bbox1_intersect = bbox1;
		bbox1_intersect.intersect_with(&bbox2).unwrap();
		assert_eq!(bbox1_intersect, TileBBox::from_min_max(4, 1, 11, 2, 12).unwrap());

		let mut bbox1_union = bbox1;
		bbox1_union.include_bbox(&bbox2).unwrap();
		assert_eq!(bbox1_union, TileBBox::from_min_max(4, 0, 10, 3, 13).unwrap());
	}

	#[test]
	fn include_tile() {
		let mut bbox = TileBBox::from_min_max(4, 0, 1, 2, 3).unwrap();
		bbox.include(4, 5);
		assert_eq!(bbox, TileBBox::from_min_max(4, 0, 1, 4, 5).unwrap());
	}

	#[test]
	fn empty_or_full() {
		let mut bbox1 = TileBBox::new_empty(12).unwrap();
		assert!(bbox1.is_empty());

		bbox1.set_full();
		assert!(bbox1.is_full());

		let mut bbox1 = TileBBox::new_full(13).unwrap();
		assert!(bbox1.is_full());

		bbox1.set_empty();
		assert!(bbox1.is_empty());
	}

	#[test]
	fn iter_coords() {
		let bbox = TileBBox::from_min_max(16, 1, 5, 2, 6).unwrap();
		let vec: Vec<TileCoord> = bbox.iter_coords().collect();
		assert_eq!(vec.len(), 4);
		assert_eq!(vec[0], TileCoord::new(16, 1, 5).unwrap());
		assert_eq!(vec[1], TileCoord::new(16, 2, 5).unwrap());
		assert_eq!(vec[2], TileCoord::new(16, 1, 6).unwrap());
		assert_eq!(vec[3], TileCoord::new(16, 2, 6).unwrap());
	}

	#[rstest]
	#[case(16, (10, 0, 0, 31, 31), "0,0,15,15 16,0,31,15 0,16,15,31 16,16,31,31")]
	#[case(16, (10, 5, 6, 25, 26), "5,6,15,15 16,6,25,15 5,16,15,26 16,16,25,26")]
	#[case(16, (10, 5, 6, 16, 16), "5,6,15,15 16,6,16,15 5,16,15,16 16,16,16,16")]
	#[case(16, (10, 5, 6, 16, 15), "5,6,15,15 16,6,16,15")]
	#[case(16, (10, 6, 7, 6, 7), "6,7,6,7")]
	#[case(64, (4, 6, 7, 6, 7), "6,7,6,7")]
	fn iter_bbox_grid_cases(#[case] size: u32, #[case] def: (u8, u32, u32, u32, u32), #[case] expected: &str) {
		let bbox = TileBBox::from_min_max(def.0, def.1, def.2, def.3, def.4).unwrap();
		let result: String = bbox
			.iter_bbox_grid(size)
			.map(|bbox| format!("{},{},{},{}", bbox.x_min, bbox.y_min, bbox.x_max(), bbox.y_max()))
			.collect::<Vec<String>>()
			.join(" ");
		assert_eq!(result, expected);
	}

	#[test]
	fn add_border() {
		let mut bbox = TileBBox::from_min_max(8, 5, 10, 20, 30).unwrap();

		// Border of (1, 1, 1, 1) should increase the size of the bbox by 1 in all directions
		bbox.expand_by(1, 1, 1, 1);
		assert_eq!(bbox, TileBBox::from_min_max(8, 4, 9, 21, 31).unwrap());

		// Border of (2, 3, 4, 5) should further increase the size of the bbox
		bbox.expand_by(2, 3, 4, 5);
		assert_eq!(bbox, TileBBox::from_min_max(8, 2, 6, 25, 36).unwrap());

		// Border of (0, 0, 0, 0) should not change the size of the bbox
		bbox.expand_by(0, 0, 0, 0);
		assert_eq!(bbox, TileBBox::from_min_max(8, 2, 6, 25, 36).unwrap());

		// Large border should saturate at max=255 for level=8
		bbox.expand_by(999, 999, 999, 999);
		assert_eq!(bbox, TileBBox::from_min_max(8, 0, 0, 255, 255).unwrap());

		// If bbox is empty, add_border should have no effect
		let mut empty_bbox = TileBBox::new_empty(8).unwrap();
		empty_bbox.expand_by(1, 2, 3, 4);
		assert_eq!(empty_bbox, TileBBox::new_empty(8).unwrap());
	}

	#[test]
	fn test_shift_by() {
		let mut bbox = TileBBox::from_min_max(4, 1, 2, 3, 4).unwrap();
		bbox.shift_by(1, 1);
		assert_eq!(bbox, TileBBox::from_min_max(4, 2, 3, 4, 5).unwrap());
	}

	#[test]
	fn test_new_empty() {
		let bbox = TileBBox::new_empty(4).unwrap();
		assert_eq!(
			bbox,
			TileBBox {
				level: 4,
				x_min: 0,
				y_min: 0,
				width: 0,
				height: 0,
			}
		);
		assert!(bbox.is_empty());
	}

	#[test]
	fn test_set_empty() {
		let mut bbox = TileBBox::from_min_max(4, 0, 0, 15, 15).unwrap();
		bbox.set_empty();
		assert!(bbox.is_empty());
	}

	#[test]
	fn test_set_full() {
		let mut bbox = TileBBox::new_empty(4).unwrap();
		bbox.set_full();
		assert!(bbox.is_full());
	}

	#[test]
	fn test_is_full() {
		let bbox = TileBBox::new_full(4).unwrap();
		assert!(bbox.is_full(), "Expected bbox ({:?}) to be full", bbox);
	}

	#[test]
	fn test_include_tile() {
		let mut bbox = TileBBox::from_min_max(6, 5, 10, 20, 30).unwrap();
		bbox.include(25, 35);
		assert_eq!(bbox, TileBBox::from_min_max(6, 5, 10, 25, 35).unwrap());
	}

	#[test]
	fn test_include_bbox() {
		let mut bbox1 = TileBBox::from_min_max(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::from_min_max(4, 1, 10, 3, 12).unwrap();
		bbox1.include_bbox(&bbox2).unwrap();
		assert_eq!(bbox1, TileBBox::from_min_max(4, 0, 10, 3, 13).unwrap());
	}

	#[test]
	fn test_intersect_bbox() {
		let mut bbox1 = TileBBox::from_min_max(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::from_min_max(4, 1, 10, 3, 12).unwrap();
		bbox1.intersect_with(&bbox2).unwrap();
		assert_eq!(bbox1, TileBBox::from_min_max(4, 1, 11, 2, 12).unwrap());
	}

	#[test]
	fn test_overlaps_bbox() {
		let bbox1 = TileBBox::from_min_max(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::from_min_max(4, 1, 10, 3, 12).unwrap();
		assert!(bbox1.overlaps_bbox(&bbox2).unwrap());

		let bbox3 = TileBBox::from_min_max(4, 8, 8, 9, 9).unwrap();
		assert!(!bbox1.overlaps_bbox(&bbox3).unwrap());
	}

	#[rstest]
	#[case((8, 100, 100, 199, 199), (8, 100, 100), 0)]
	#[case((8, 100, 100, 199, 199), (8, 101, 100), 1)]
	#[case((8, 100, 100, 199, 199), (8, 199, 100), 99)]
	#[case((8, 100, 100, 199, 199), (8, 100, 101), 100)]
	#[case((8, 100, 100, 199, 199), (8, 100, 199), 9900)]
	#[case((8, 100, 100, 199, 199), (8, 199, 199), 9999)]
	fn get_tile_index_cases(
		#[case] bbox: (u8, u32, u32, u32, u32),
		#[case] coord: (u8, u32, u32),
		#[case] expected: u64,
	) {
		let (l, x0, y0, x1, y1) = bbox;
		let bbox = TileBBox::from_min_max(l, x0, y0, x1, y1).unwrap();
		let (cl, cx, cy) = coord;
		let tc = TileCoord::new(cl, cx, cy).unwrap();
		assert_eq!(bbox.index_of(&tc).unwrap(), expected);
	}

	#[test]
	fn test_as_geo_bbox() {
		let bbox = TileBBox::from_min_max(4, 5, 10, 7, 12).unwrap();
		let geo_bbox = bbox.to_geo_bbox();
		assert_eq!(
			geo_bbox.as_string_list(),
			"-67.5,-74.01954331150228,0,-40.97989806962013"
		);
	}

	#[test]
	fn test_contains() {
		let bbox = TileBBox::from_min_max(4, 5, 10, 7, 12).unwrap();
		assert!(bbox.contains(&TileCoord::new(4, 6, 11).unwrap()));
		assert!(!bbox.contains(&TileCoord::new(4, 4, 9).unwrap()));
		assert!(!bbox.contains(&TileCoord::new(5, 6, 11).unwrap()));
	}

	#[test]
	fn test_new_valid_bbox() {
		let bbox = TileBBox::from_min_max(6, 5, 10, 15, 20).unwrap();
		assert_eq!(bbox.level, 6);
		assert_eq!(bbox.x_min, 5);
		assert_eq!(bbox.y_min, 10);
		assert_eq!(bbox.x_max(), 15);
		assert_eq!(bbox.y_max(), 20);
	}

	#[test]
	fn test_new_invalid_level() {
		let result = TileBBox::from_min_max(32, 0, 0, 1, 1);
		assert!(result.is_err());
	}

	#[test]
	fn test_new_invalid_coordinates() {
		let result = TileBBox::from_min_max(4, 10, 10, 5, 15);
		assert!(result.is_err());

		let result = TileBBox::from_min_max(4, 5, 15, 7, 10);
		assert!(result.is_err());

		let result = TileBBox::from_min_max(4, 0, 0, 16, 15); // x_max exceeds max for level 4
		assert!(result.is_err());
	}

	#[test]
	fn test_new_full() {
		let bbox = TileBBox::new_full(4).unwrap();
		assert_eq!(bbox, TileBBox::from_min_max(4, 0, 0, 15, 15).unwrap());
		assert!(bbox.is_full());
	}

	#[test]
	fn test_from_geo_valid() {
		let geo_bbox = GeoBBox(-180.0, -85.05112878, 180.0, 85.05112878);
		let bbox = TileBBox::from_geo(2, &geo_bbox).unwrap();
		assert_eq!(bbox, TileBBox::from_min_max(2, 0, 0, 3, 3).unwrap());
	}

	#[test]
	fn test_from_geo_invalid() {
		let geo_bbox = GeoBBox(-200.0, -100.0, 200.0, 100.0); // Invalid geo coordinates
		let result = TileBBox::from_geo(2, &geo_bbox);
		assert!(result.is_err());
	}

	#[test]
	fn test_is_empty() {
		let empty_bbox = TileBBox::new_empty(4).unwrap();
		assert!(empty_bbox.is_empty());

		let non_empty_bbox = TileBBox::from_min_max(6, 5, 10, 15, 20).unwrap();
		assert!(!non_empty_bbox.is_empty());
	}

	#[test]
	fn test_width_height() {
		let bbox = TileBBox::from_min_max(6, 5, 10, 15, 20).unwrap();
		assert_eq!(bbox.width(), 11);
		assert_eq!(bbox.height(), 11);

		let empty_bbox = TileBBox::new_empty(4).unwrap();
		assert_eq!(empty_bbox.width(), 0);
		assert_eq!(empty_bbox.height(), 0);
	}

	#[test]
	fn test_count_tiles() {
		let bbox = TileBBox::from_min_max(6, 5, 10, 15, 20).unwrap();
		assert_eq!(bbox.count_tiles(), 121);

		let empty_bbox = TileBBox::new_empty(4).unwrap();
		assert_eq!(empty_bbox.count_tiles(), 0);
	}

	#[test]
	fn test_include() -> Result<()> {
		let mut bbox = TileBBox::new_empty(6)?;
		bbox.include(5, 10);
		assert_eq!(bbox, TileBBox::from_min_max(6, 5, 10, 5, 10).unwrap());

		bbox.include(15, 20);
		assert_eq!(bbox, TileBBox::from_min_max(6, 5, 10, 15, 20).unwrap());

		bbox.include(10, 15);
		assert_eq!(bbox, TileBBox::from_min_max(6, 5, 10, 15, 20).unwrap());

		Ok(())
	}

	#[test]
	fn test_include_coord() -> Result<()> {
		let mut bbox = TileBBox::new_empty(6)?;
		let coord = TileCoord::new(6, 5, 10).unwrap();
		bbox.include_coord(&coord)?;
		assert_eq!(bbox, TileBBox::from_min_max(6, 5, 10, 5, 10).unwrap());

		let coord = TileCoord::new(6, 15, 20).unwrap();
		bbox.include_coord(&coord)?;
		assert_eq!(bbox, TileBBox::from_min_max(6, 5, 10, 15, 20).unwrap());

		// Attempt to include a coordinate with a different zoom level
		let coord_invalid = TileCoord::new(5, 10, 15).unwrap();
		let result = bbox.include_coord(&coord_invalid);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn test_add_border() -> Result<()> {
		let mut bbox = TileBBox::from_min_max(6, 5, 10, 15, 20)?;

		// Add a border within bounds
		bbox.expand_by(2, 3, 2, 3);
		assert_eq!(bbox, TileBBox::from_min_max(6, 3, 7, 17, 23).unwrap());

		// Add a border that exceeds bounds, should clamp to max
		bbox.expand_by(10, 10, 10, 10);
		assert_eq!(bbox, TileBBox::from_min_max(6, 0, 0, 27, 33).unwrap());

		// Add border to an empty bounding box, should have no effect
		let mut empty_bbox = TileBBox::new_empty(6)?;
		empty_bbox.expand_by(1, 1, 1, 1);
		assert!(empty_bbox.is_empty());

		// Attempt to add a border with zero values
		bbox.expand_by(0, 0, 0, 0);
		assert_eq!(bbox, TileBBox::from_min_max(6, 0, 0, 27, 33).unwrap());

		Ok(())
	}

	#[test]
	fn should_include_bbox_correctly_with_valid_and_empty_bboxes() -> Result<()> {
		let mut bbox1 = TileBBox::from_min_max(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::from_min_max(6, 10, 15, 20, 25)?;

		bbox1.include_bbox(&bbox2)?;
		assert_eq!(bbox1, TileBBox::from_min_max(6, 5, 10, 20, 25).unwrap());

		// Including an empty bounding box should have no effect
		let empty_bbox = TileBBox::new_empty(6)?;
		bbox1.include_bbox(&empty_bbox)?;
		assert_eq!(bbox1, TileBBox::from_min_max(6, 5, 10, 20, 25).unwrap());

		// Attempting to include a bounding box with different zoom level
		let bbox_diff_level = TileBBox::from_min_max(5, 5, 10, 20, 25)?;
		let result = bbox1.include_bbox(&bbox_diff_level);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_intersect_bboxes_correctly_and_handle_empty_and_different_levels() -> Result<()> {
		let mut bbox1 = TileBBox::from_min_max(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::from_min_max(6, 10, 15, 20, 25)?;

		bbox1.intersect_with(&bbox2)?;
		assert_eq!(bbox1, TileBBox::from_min_max(6, 10, 15, 15, 20).unwrap());

		// Intersect with a non-overlapping bounding box
		let bbox3 = TileBBox::from_min_max(6, 16, 21, 20, 25)?;
		bbox1.intersect_with(&bbox3)?;
		assert!(bbox1.is_empty());

		// Attempting to intersect with a bounding box of different zoom level
		let bbox_diff_level = TileBBox::from_min_max(5, 10, 15, 15, 20)?;
		let result = bbox1.intersect_with(&bbox_diff_level);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_correctly_determine_bbox_overlap() -> Result<()> {
		let bbox1 = TileBBox::from_min_max(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::from_min_max(6, 10, 15, 20, 25)?;
		let bbox3 = TileBBox::from_min_max(6, 16, 21, 20, 25)?;
		let bbox4 = TileBBox::from_min_max(5, 10, 15, 15, 20)?;

		assert!(bbox1.overlaps_bbox(&bbox2)?);
		assert!(!bbox1.overlaps_bbox(&bbox3)?);
		assert!(bbox1.overlaps_bbox(&bbox1)?);
		assert!(bbox1.overlaps_bbox(&bbox1.clone())?);

		// Overlaps with a bounding box of different zoom level
		let result = bbox1.overlaps_bbox(&bbox4);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_get_correct_tile_index() -> Result<()> {
		let bbox = TileBBox::from_min_max(4, 5, 10, 7, 12)?;

		assert_eq!(bbox.index_of(&TileCoord::new(4, 5, 10).unwrap()).unwrap(), 0);
		assert_eq!(bbox.index_of(&TileCoord::new(4, 6, 10).unwrap()).unwrap(), 1);
		assert_eq!(bbox.index_of(&TileCoord::new(4, 7, 10).unwrap()).unwrap(), 2);
		assert_eq!(bbox.index_of(&TileCoord::new(4, 5, 11).unwrap()).unwrap(), 3);
		assert_eq!(bbox.index_of(&TileCoord::new(4, 7, 12).unwrap()).unwrap(), 8);

		// Attempt to get index of a coordinate outside the bounding box
		let coord_outside = TileCoord::new(4, 4, 9).unwrap();
		let result = bbox.index_of(&coord_outside);
		assert!(result.is_err());

		// Attempt to get index with mismatched zoom level
		let coord_diff_level = TileCoord::new(5, 5, 10).unwrap();
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
	fn get_coord_by_index_cases(#[case] index: u64, #[case] coord: (u8, u32, u32)) {
		let bbox = TileBBox::from_min_max(4, 5, 10, 7, 12).unwrap();
		let (l, x, y) = coord;
		assert_eq!(bbox.coord_at_index(index).unwrap(), TileCoord::new(l, x, y).unwrap());
	}

	#[test]
	fn get_coord_by_index_out_of_bounds() {
		let bbox = TileBBox::from_min_max(4, 5, 10, 7, 12).unwrap();
		assert!(bbox.coord_at_index(9).is_err());
	}

	#[test]
	fn should_convert_to_geo_bbox_correctly() -> Result<()> {
		let bbox = TileBBox::from_min_max(4, 5, 10, 7, 12)?;
		let geo_bbox = bbox.to_geo_bbox();

		// Assuming TileCoord::as_geo() converts tile coordinates to geographical coordinates correctly,
		// the following is an example expected output. Adjust based on actual implementation.
		// For demonstration, let's assume:
		// - Tile (5, 10, 4) maps to longitude -67.5 and latitude 74.01954331
		// - Tile (7, 12, 4) maps to longitude 0.0 and latitude 40.97989807
		let expected_geo_bbox = GeoBBox(-67.5, -74.01954331150228, 0.0, -40.97989806962013);
		assert_eq!(geo_bbox, expected_geo_bbox);

		Ok(())
	}

	#[test]
	fn should_determine_contains3_correctly() -> Result<()> {
		let bbox = TileBBox::from_min_max(4, 5, 10, 7, 12)?;
		let valid_coord = TileCoord::new(4, 6, 11).unwrap();
		let invalid_coord_zoom = TileCoord::new(5, 6, 11).unwrap();
		let invalid_coord_outside = TileCoord::new(4, 4, 9).unwrap();

		assert!(bbox.contains(&valid_coord));
		assert!(!bbox.contains(&invalid_coord_zoom));
		assert!(!bbox.contains(&invalid_coord_outside));

		Ok(())
	}

	#[test]
	fn should_iterate_over_coords_correctly() -> Result<()> {
		let bbox = TileBBox::from_min_max(4, 5, 10, 6, 11)?;
		let coords: Vec<TileCoord> = bbox.iter_coords().collect();
		let expected_coords = vec![
			TileCoord::new(4, 5, 10).unwrap(),
			TileCoord::new(4, 6, 10).unwrap(),
			TileCoord::new(4, 5, 11).unwrap(),
			TileCoord::new(4, 6, 11).unwrap(),
		];
		assert_eq!(coords, expected_coords);

		Ok(())
	}

	#[test]
	fn should_iterate_over_coords_correctly_when_consumed() -> Result<()> {
		let bbox = TileBBox::from_min_max(4, 5, 10, 6, 11)?;
		let coords: Vec<TileCoord> = bbox.into_iter_coords().collect();
		let expected_coords = vec![
			TileCoord::new(4, 5, 10).unwrap(),
			TileCoord::new(4, 6, 10).unwrap(),
			TileCoord::new(4, 5, 11).unwrap(),
			TileCoord::new(4, 6, 11).unwrap(),
		];
		assert_eq!(coords, expected_coords);

		Ok(())
	}

	#[test]
	fn should_split_bbox_into_correct_grid() -> Result<()> {
		let bbox = TileBBox::from_min_max(4, 0, 0, 7, 7)?;

		let grid_size = 4;
		let grids: Vec<TileBBox> = bbox.iter_bbox_grid(grid_size).collect();

		let expected_grids = vec![
			TileBBox::from_min_max(4, 0, 0, 3, 3)?,
			TileBBox::from_min_max(4, 4, 0, 7, 3)?,
			TileBBox::from_min_max(4, 0, 4, 3, 7)?,
			TileBBox::from_min_max(4, 4, 4, 7, 7)?,
		];

		assert_eq!(grids, expected_grids);

		Ok(())
	}

	#[test]
	fn should_scale_down_correctly() -> Result<()> {
		let mut bbox = TileBBox::from_min_max(4, 4, 4, 7, 7)?;
		bbox.scale_down(2);
		assert_eq!(bbox, TileBBox::from_min_max(4, 2, 2, 3, 3)?);

		// Scaling down by a factor larger than the coordinates
		bbox.scale_down(4);
		assert_eq!(bbox, TileBBox::from_min_max(4, 0, 0, 0, 0)?);

		Ok(())
	}

	#[test]
	fn test_scaled_down_returns_new_bbox_and_preserves_original() -> Result<()> {
		// Original bbox
		let original = TileBBox::from_min_max(5, 10, 15, 20, 25)?;
		// scaled_down should return a new bbox without modifying the original
		let scaled = original.scaled_down(4);
		// Coordinates divided by 4: 10/4=2,15/4=3,20/4=5,25/4=6
		assert_eq!(scaled, TileBBox::from_min_max(5, 2, 3, 5, 6)?);
		// Original remains unchanged
		assert_eq!(original, TileBBox::from_min_max(5, 10, 15, 20, 25)?);
		// Scaling by 1 should produce identical bbox
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
	fn test_scale_down_cases(#[case] args: (u32, u32, u32, u32)) {
		let (min0, max0, min1, max1) = args;
		let mut bbox0 = TileBBox::from_min_max(8, min0, min0, max0, max0).unwrap();
		let bbox1 = TileBBox::from_min_max(8, min1, min1, max1, max1).unwrap();
		assert_eq!(
			bbox0.scaled_down(4),
			bbox1,
			"scaled_down(4) of {bbox0:?} should return {bbox1:?}"
		);
		bbox0.scale_down(4);
		assert_eq!(bbox0, bbox1, "scale_down(4) of {bbox0:?} should result in {bbox1:?}");
	}

	#[test]
	fn should_shift_bbox_correctly() -> Result<()> {
		let mut bbox = TileBBox::from_min_wh(6, 5, 10, 10, 10)?;
		bbox.shift_by(3, 4);
		assert_eq!(bbox, TileBBox::from_min_wh(6, 8, 14, 10, 10)?);

		// Shifting beyond max should not cause overflow due to saturating_add
		let mut bbox = TileBBox::from_min_wh(6, 14, 14, 10, 10)?;
		bbox.shift_by(2, 2);
		assert_eq!(bbox, TileBBox::from_min_wh(6, 16, 16, 10, 10)?);

		let mut bbox = TileBBox::from_min_wh(6, 5, 10, 10, 10)?;
		bbox.shift_by(-3, -5);
		assert_eq!(bbox, TileBBox::from_min_wh(6, 2, 5, 10, 10)?);

		// Subtracting more than current coordinates should saturate at 0
		bbox.shift_by(-5, -10);
		assert_eq!(bbox, TileBBox::from_min_wh(6, 0, 0, 10, 10)?);

		Ok(())
	}

	#[test]
	fn should_handle_bbox_overlap_edge_cases() -> Result<()> {
		let bbox1 = TileBBox::from_min_max(4, 0, 0, 5, 5)?;
		let bbox2 = TileBBox::from_min_max(4, 5, 5, 10, 10)?;
		let bbox3 = TileBBox::from_min_max(4, 6, 6, 10, 10)?;
		let bbox4 = TileBBox::from_min_max(4, 0, 0, 5, 5)?;

		// Overlapping at the edge
		assert!(bbox1.overlaps_bbox(&bbox2)?);

		// No overlapping
		assert!(!bbox1.overlaps_bbox(&bbox3)?);

		// Completely overlapping
		assert!(bbox1.overlaps_bbox(&bbox4)?);

		// One empty bounding box
		let empty_bbox = TileBBox::new_empty(4)?;
		assert!(!bbox1.overlaps_bbox(&empty_bbox)?);

		Ok(())
	}

	#[test]
	fn should_handle_empty_bbox_in_grid_iteration() -> Result<()> {
		let bbox = TileBBox::new_empty(4)?;
		let grids: Vec<TileBBox> = bbox.iter_bbox_grid(4).collect();
		assert!(grids.is_empty());
		Ok(())
	}

	#[test]
	fn should_handle_single_tile_in_grid_iteration() -> Result<()> {
		let bbox = TileBBox::from_min_max(4, 5, 10, 5, 10)?;
		let grids: Vec<TileBBox> = bbox.iter_bbox_grid(4).collect();
		let expected_grids = vec![TileBBox::from_min_max(4, 5, 10, 5, 10).unwrap()];
		assert_eq!(grids, expected_grids);
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
	fn test_round_shifting_cases(#[case] inp: [u32; 4], #[case] exp: [u32; 4]) {
		let bbox_exp = TileBBox::from_min_max(8, exp[0], exp[1], exp[2], exp[3]).unwrap();
		let mut bbox_inp = TileBBox::from_min_max(8, inp[0], inp[1], inp[2], inp[3]).unwrap();
		assert_eq!(bbox_inp.rounded(4), bbox_exp);
		bbox_inp.round(4);
		assert_eq!(bbox_inp, bbox_exp);
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
	fn test_round_scaling_cases(#[case] scale: u32, #[case] exp: [u32; 4]) {
		let bbox_exp = TileBBox::from_min_max(12, exp[0], exp[1], exp[2], exp[3]).unwrap();
		let mut bbox_inp = TileBBox::from_min_max(12, 12, 34, 56, 78).unwrap();
		assert_eq!(bbox_inp.rounded(scale), bbox_exp);
		bbox_inp.round(scale);
		assert_eq!(bbox_inp, bbox_exp);
	}

	#[rstest]
	#[case((1, 0, 0, 1, 1), (1, 0, 0, 1, 1))]
	#[case((2, 0, 0, 1, 1), (2, 0, 2, 1, 3))]
	#[case((3, 0, 0, 1, 1), (3, 0, 6, 1, 7))]
	#[case((9, 10, 0, 10, 511), (9, 10, 0, 10, 511))]
	#[case((9, 0, 10, 511, 10), (9, 0, 501, 511, 501))]
	fn bbox_flip_y(#[case] a: (u8, u32, u32, u32, u32), #[case] b: (u8, u32, u32, u32, u32)) {
		let mut t = TileBBox::from_min_max(a.0, a.1, a.2, a.3, a.4).unwrap();
		t.flip_y();

		assert_eq!(t, TileBBox::from_min_max(b.0, b.1, b.2, b.3, b.4).unwrap());
	}

	#[test]
	fn bbox_swap_xy_transform() {
		let mut bbox = TileBBox::from_min_max(4, 1, 2, 3, 4).unwrap();
		bbox.swap_xy();
		assert_eq!(bbox, TileBBox::from_min_max(4, 2, 1, 4, 3).unwrap());
	}

	#[test]
	fn set_width_height_clamp_to_bounds() {
		// level 4 → max coordinate = 15
		let mut bbox = TileBBox::from_min_wh(4, 10, 10, 3, 3).unwrap(); // covers x=10..12, y=10..12
		bbox.set_width(10); // would exceed max → clamp to 10..15 → width = 6
		bbox.set_height(10);
		assert_eq!(bbox.x_min(), 10);
		assert_eq!(bbox.y_min(), 10);
		assert_eq!(bbox.x_max(), 15);
		assert_eq!(bbox.y_max(), 15);
	}

	#[test]
	fn set_min_max_keep_consistency() {
		let mut bbox = TileBBox::from_min_max(5, 8, 9, 12, 13).unwrap(); // width=5, height=5
		// Move min right/up; max should remain the same
		bbox.set_x_min(10);
		bbox.set_y_min(11);
		assert_eq!(bbox.x_min(), 10);
		assert_eq!(bbox.y_min(), 11);
		assert_eq!(bbox.x_max(), 12);
		assert_eq!(bbox.y_max(), 13);
		// Move max left/down; min should remain the same
		bbox.set_x_max(11);
		bbox.set_y_max(12);
		assert_eq!(bbox.x_min(), 10);
		assert_eq!(bbox.y_min(), 11);
		assert_eq!(bbox.x_max(), 11);
		assert_eq!(bbox.y_max(), 12);
		// Setting max less than min should empty the dimension
		bbox.set_x_max(9);
		bbox.set_y_max(10);
		assert_eq!(bbox.width(), 0);
		assert_eq!(bbox.height(), 0);
	}

	#[test]
	fn shift_to_clamps_to_edge() {
		let mut bbox = TileBBox::from_min_max(3, 4, 4, 6, 6).unwrap(); // level 3 → max=7
		// x_max would be 9 without clamping; expect clamp to 7 and maintain width
		bbox.shift_to(6, 6);
		assert_eq!(bbox.x_min(), 6);
		assert_eq!(bbox.y_min(), 6);
		assert_eq!(bbox.x_max(), 7);
		assert_eq!(bbox.y_max(), 7);
	}

	#[rstest]
	#[case(4, 6, 2, 3)]
	#[case(5, 6, 2, 3)]
	#[case(4, 7, 2, 3)]
	#[case(5, 7, 2, 3)]
	fn level_decrease(#[case] min_in: u32, #[case] max_in: u32, #[case] min_out: u32, #[case] max_out: u32) {
		let mut bbox = TileBBox::from_min_max(10, min_in, min_in, max_in, max_in).unwrap();
		bbox.level_down();
		assert_eq!(bbox.level, 9);
		assert_eq!(bbox.x_min(), min_out);
		assert_eq!(bbox.y_min(), min_out);
		assert_eq!(bbox.x_max(), max_out);
		assert_eq!(bbox.y_max(), max_out);
	}

	#[rstest]
	#[case(4, 6, 8, 13)]
	#[case(5, 6, 10, 13)]
	#[case(4, 7, 8, 15)]
	#[case(5, 7, 10, 15)]
	fn level_increase(#[case] min_in: u32, #[case] max_in: u32, #[case] min_out: u32, #[case] max_out: u32) {
		let mut bbox = TileBBox::from_min_max(10, min_in, min_in, max_in, max_in).unwrap();
		bbox.level_up();
		assert_eq!(bbox.level, 11);
		assert_eq!(bbox.x_min(), min_out);
		assert_eq!(bbox.y_min(), min_out);
		assert_eq!(bbox.x_max(), max_out);
		assert_eq!(bbox.y_max(), max_out);
	}

	#[test]
	fn level_increase_decrease_roundtrip() {
		let original = TileBBox::from_min_max(4, 5, 6, 7, 8).unwrap();
		let inc = original.leveled_up();
		assert_eq!(inc.level, 5);
		assert_eq!(inc.x_min(), 10);
		assert_eq!(inc.y_min(), 12);
		assert_eq!(inc.x_max(), 15);
		assert_eq!(inc.y_max(), 17);
		let dec = inc.leveled_down();
		assert_eq!(dec, original);
	}

	#[rstest]
	#[case(4, 5, 6, 7, 8, 3, 3)]
	#[case(8, 0, 0, 0, 0, 1, 1)]
	fn corners_and_dimensions(
		#[case] level: u8,
		#[case] x0: u32,
		#[case] y0: u32,
		#[case] x1: u32,
		#[case] y1: u32,
		#[case] width: u32,
		#[case] height: u32,
	) {
		let bbox = TileBBox::from_min_max(level, x0, y0, x1, y1).unwrap();
		assert_eq!(bbox.min_corner(), TileCoord::new(level, x0, y0).unwrap());
		assert_eq!(bbox.max_corner(), TileCoord::new(level, x1, y1).unwrap());
		assert_eq!(bbox.dimensions(), (width, height));
	}

	#[rstest]
	#[case(4, 0, 1, 1, 1)]
	#[case(5, 1, 2, 3, 3)]
	#[case(6, 3, 5, 6, 7)]
	#[case(7, 6, 10, 13, 15)]
	#[case(8, 12, 20, 27, 31)]
	fn as_level_up_and_down(#[case] level: u32, #[case] x0: u32, #[case] y0: u32, #[case] x1: u32, #[case] y1: u32) {
		let bbox = TileBBox::from_min_max(6, 3, 5, 6, 7).unwrap();
		let up = bbox.at_level(level as u8);
		assert_eq!(
			[up.level as u32, up.x_min(), up.y_min(), up.x_max(), up.y_max()],
			[level, x0, y0, x1, y1]
		);
	}

	#[test]
	fn get_quadrant_happy_path() -> Result<()> {
		let bbox = TileBBox::from_min_max(4, 8, 12, 11, 15).unwrap(); // 4x4 → even
		assert_eq!(bbox.get_quadrant(0)?, TileBBox::from_min_max(4, 8, 12, 9, 13)?);
		assert_eq!(bbox.get_quadrant(1)?, TileBBox::from_min_max(4, 10, 12, 11, 13)?);
		assert_eq!(bbox.get_quadrant(2)?, TileBBox::from_min_max(4, 8, 14, 9, 15)?);
		assert_eq!(bbox.get_quadrant(3)?, TileBBox::from_min_max(4, 10, 14, 11, 15)?);
		Ok(())
	}

	#[test]
	fn get_quadrant_errors() {
		// Empty bbox → Ok(empty)
		let empty = TileBBox::new_empty(4).unwrap();
		assert!(empty.get_quadrant(0).unwrap().is_empty());
		// Odd width/height → error
		let odd_w = TileBBox::from_min_max(4, 0, 0, 2, 3).unwrap(); // width=3
		assert!(odd_w.get_quadrant(0).is_err());
		let odd_h = TileBBox::from_min_max(4, 0, 0, 3, 2).unwrap(); // height=3
		assert!(odd_h.get_quadrant(0).is_err());
		// Invalid quadrant index
		let even = TileBBox::from_min_max(4, 0, 0, 3, 3).unwrap();
		assert!(even.get_quadrant(4).is_err());
	}

	#[test]
	fn max_value_and_string() {
		let bbox = TileBBox::from_min_max(5, 1, 2, 3, 4).unwrap();
		assert_eq!(bbox.max_coord_at_level(), (1u32 << 5) - 1);
		assert_eq!(bbox.as_string(), "5:[1,2,3,4]");
	}
}
