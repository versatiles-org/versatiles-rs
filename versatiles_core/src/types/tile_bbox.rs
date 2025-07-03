//! This module defines the `TileBBox` struct, representing a bounding box for tiles at a specific zoom level.
//! It provides methods to create, manipulate, and query these bounding boxes.
//!
//! # Overview
//!
//! The `TileBBox` struct is used to define a rectangular area of tiles within a specific zoom level.
//! It supports operations such as inclusion, intersection, scaling, and iteration over tile coordinates.
//! This is particularly useful in mapping applications where tile management is essential.

use super::{GeoBBox, TileBBoxPyramid, TileCoord2, TileCoord3};
use anyhow::{ensure, Result};
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TileBBox {
	/// Zoom level of the bounding box.
	pub level: u8,
	/// Minimum x-coordinate.
	pub x_min: u32,
	/// Minimum y-coordinate.
	pub y_min: u32,
	/// Maximum x-coordinate.
	pub x_max: u32,
	/// Maximum y-coordinate.
	pub y_max: u32,
	/// Maximum valid coordinate based on zoom level.
	pub max: u32,
}

#[allow(dead_code)]
impl TileBBox {
	// -------------------------------------------------------------------------
	// Constructors
	// -------------------------------------------------------------------------

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
	pub fn new(level: u8, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");

		let max = 2u32.pow(level as u32) - 1;

		ensure!(x_max <= max, "x_max ({x_max}) must be <= max ({max})");
		ensure!(y_max <= max, "y_max ({y_max}) must be <= max ({max})");
		ensure!(x_min <= x_max, "x_min ({x_min}) must be <= x_max ({x_max})");
		ensure!(y_min <= y_max, "y_min ({y_min}) must be <= y_max ({y_max})");

		let bbox = TileBBox {
			level,
			max,
			x_min,
			y_min,
			x_max,
			y_max,
		};

		Ok(bbox)
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
		let max = 2u32.pow(level as u32) - 1;
		Self::new(level, 0, 0, max, max)
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
		let max = 2u32.pow(level as u32) - 1;
		Ok(TileBBox {
			level,
			max,
			x_min: max + 1,
			y_min: max + 1,
			x_max: 0,
			y_max: 0,
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
		let p_min = TileCoord2::from_geo(bbox.0, bbox.3, level, false)?;
		let p_max = TileCoord2::from_geo(bbox.2, bbox.1, level, true)?;

		Self::new(level, p_min.x, p_min.y, p_max.x, p_max.y)
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
		(self.x_max < self.x_min) || (self.y_max < self.y_min)
	}

	/// Calculates the width (in tiles) of the bounding box.
	///
	/// # Returns
	///
	/// * Width as `u32`.
	/// * `0` if the bounding box is empty.
	pub fn width(&self) -> u32 {
		if self.x_max < self.x_min {
			0
		} else {
			self.x_max - self.x_min + 1
		}
	}

	/// Calculates the height (in tiles) of the bounding box.
	///
	/// # Returns
	///
	/// * Height as `u32`.
	/// * `0` if the bounding box is empty.
	pub fn height(&self) -> u32 {
		if self.y_max < self.y_min {
			0
		} else {
			self.y_max - self.y_min + 1
		}
	}

	/// Counts the total number of tiles within the bounding box.
	///
	/// # Returns
	///
	/// * Number of tiles as `u64`.
	/// * `0` if the bounding box is empty.
	pub fn count_tiles(&self) -> u64 {
		(self.width() as u64) * (self.height() as u64)
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
		!self.is_empty() && self.x_min == 0 && self.y_min == 0 && self.x_max == self.max && self.y_max == self.max
	}

	// -------------------------------------------------------------------------
	// Containment Checks
	// -------------------------------------------------------------------------

	/// Checks if the bounding box contains a specific tile coordinate (`TileCoord2`).
	///
	/// # Arguments
	///
	/// * `coord` - Reference to the tile coordinate to check.
	///
	/// # Returns
	///
	/// * `true` if the coordinate is within the bounding box.
	/// * `false` otherwise.
	pub fn contains2(&self, coord: &TileCoord2) -> bool {
		coord.x >= self.x_min && coord.x <= self.x_max && coord.y >= self.y_min && coord.y <= self.y_max
	}

	/// Checks if the bounding box contains a specific tile coordinate (`TileCoord3`) at the same zoom level.
	///
	/// # Arguments
	///
	/// * `coord` - Reference to the tile coordinate to check.
	///
	/// # Returns
	///
	/// * `true` if the coordinate is within the bounding box and at the same zoom level.
	/// * `false` otherwise.
	pub fn contains3(&self, coord: &TileCoord3) -> bool {
		coord.z == self.level
			&& coord.x >= self.x_min
			&& coord.x <= self.x_max
			&& coord.y >= self.y_min
			&& coord.y <= self.y_max
	}

	// -------------------------------------------------------------------------
	// Mutation Methods
	// -------------------------------------------------------------------------

	/// Sets the bounding box to an empty state.
	///
	/// After calling this method, `is_empty()` will return `true`.
	pub fn set_empty(&mut self) {
		self.x_min = 1;
		self.y_min = 1;
		self.x_max = 0;
		self.y_max = 0;
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
		self.x_min = 0;
		self.y_min = 0;
		self.x_max = self.max;
		self.y_max = self.max;
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
	pub fn include_coord(&mut self, x: u32, y: u32) {
		if self.is_empty() {
			// Initialize bounding box to the provided coordinate
			self.x_min = x;
			self.y_min = y;
			self.x_max = x;
			self.y_max = y;
		} else {
			// Expand bounding box to include the new coordinate
			self.x_min = self.x_min.min(x);
			self.y_min = self.y_min.min(y);
			self.x_max = self.x_max.max(x).min(self.max);
			self.y_max = self.y_max.max(y).min(self.max);
		}
	}

	/// Includes a tile coordinate (`TileCoord3`) within the bounding box.
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
	pub fn include_coord3(&mut self, coord: &TileCoord3) -> Result<()> {
		if coord.z != self.level {
			return Err(anyhow::anyhow!(
				"Cannot include TileCoord3 with z={} into TileBBox at z={}",
				coord.z,
				self.level
			));
		}
		self.include_coord(coord.x, coord.y);
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
	pub fn add_border(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) {
		if !self.is_empty() {
			self.x_min = self.x_min.saturating_sub(x_min);
			self.y_min = self.y_min.saturating_sub(y_min);
			self.x_max = (self.x_max + x_max).min(self.max);
			self.y_max = (self.y_max + y_max).min(self.max);
		}
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
		if self.level != bbox.level {
			return Err(anyhow::anyhow!(
				"Cannot include TileBBox with level={} into TileBBox with level={}",
				bbox.level,
				self.level
			));
		}

		if !bbox.is_empty() {
			if self.is_empty() {
				// If current bounding box is empty, adopt the other bounding box
				*self = *bbox;
			} else {
				// Expand to include the other bounding box
				self.x_min = self.x_min.min(bbox.x_min);
				self.y_min = self.y_min.min(bbox.y_min);
				self.x_max = self.x_max.max(bbox.x_max).min(self.max);
				self.y_max = self.y_max.max(bbox.y_max).min(self.max);
			}
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
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		if self.level != bbox.level {
			return Err(anyhow::anyhow!(
				"Cannot intersect TileBBox with level={} with TileBBox with level={}",
				bbox.level,
				self.level
			));
		}

		if !self.is_empty() && !bbox.is_empty() {
			self.x_min = self.x_min.max(bbox.x_min);
			self.y_min = self.y_min.max(bbox.y_min);
			self.x_max = self.x_max.min(bbox.x_max);
			self.y_max = self.y_max.min(bbox.y_max);
		} else {
			// If either bounding box is empty, the intersection is empty
			self.set_empty();
		}

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
	///
	/// # Returns
	///
	/// * `Ok(())` if intersection is successful.
	/// * `Err(anyhow::Error)` if any validation fails.
	pub fn intersect_pyramid(&mut self, pyramid: &TileBBoxPyramid) -> Result<()> {
		let pyramid_bbox = pyramid.get_level_bbox(self.level);
		self.intersect_bbox(pyramid_bbox)
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

		Ok(self.x_min <= bbox.x_max && self.x_max >= bbox.x_min && self.y_min <= bbox.y_max && self.y_max >= bbox.y_min)
	}

	// -------------------------------------------------------------------------
	// Coordinate Transformations
	// -------------------------------------------------------------------------

	/// Converts the bounding box to geographical coordinates (`GeoBBox`).
	///
	/// # Returns
	///
	/// * `GeoBBox` representing the geographical area covered by this bounding box.
	pub fn as_geo_bbox(&self) -> GeoBBox {
		// Top-left in geospatial terms is (x_min, y_max + 1)
		let p_min = TileCoord3::new(self.x_min, self.y_max + 1, self.level)
			.unwrap()
			.as_geo();
		// Bottom-right in geospatial terms is (x_max + 1, y_min)
		let p_max = TileCoord3::new(self.x_max + 1, self.y_min, self.level)
			.unwrap()
			.as_geo();

		GeoBBox(p_min[0], p_min[1], p_max[0], p_max[1])
	}

	/// Shifts the bounding box by specified amounts in the x and y directions.
	///
	/// Adds `x` to both `x_min` and `x_max`, and `y` to both `y_min` and `y_max`.
	///
	/// # Arguments
	///
	/// * `x` - Amount to shift in the x-direction.
	/// * `y` - Amount to shift in the y-direction.
	///
	/// # Returns
	///
	/// * `Ok(())` if the shift is successful.
	/// * `Err(anyhow::Error)` if the resulting bounding box is invalid.
	pub fn shift_by(&mut self, x: u32, y: u32) {
		self.x_min = self.x_min.saturating_add(x);
		self.y_min = self.y_min.saturating_add(y);
		self.x_max = self.x_max.saturating_add(x);
		self.y_max = self.y_max.saturating_add(y);
	}

	/// Subtracts a tile coordinate (`TileCoord2`) from the bounding box, saturating at 0.
	///
	/// This effectively moves the bounding box left/up by the specified amounts.
	///
	/// # Arguments
	///
	/// * `c` - Reference to the tile coordinate containing the amounts to subtract.
	///
	/// # Returns
	///
	/// * `Ok(())` if the subtraction is successful.
	/// * `Err(anyhow::Error)` if the resulting bounding box is invalid.
	pub fn subtract_coord2(&mut self, c: &TileCoord2) {
		self.x_min = self.x_min.saturating_sub(c.x);
		self.y_min = self.y_min.saturating_sub(c.y);
		self.x_max = self.x_max.saturating_sub(c.x);
		self.y_max = self.y_max.saturating_sub(c.y);
	}

	/// Subtracts specified amounts from the bounding box, saturating at 0.
	///
	/// This effectively moves the bounding box left/up by `x` and `y` respectively.
	///
	/// # Arguments
	///
	/// * `x` - Amount to subtract from the x-coordinates.
	/// * `y` - Amount to subtract from the y-coordinates.
	///
	/// # Returns
	///
	/// * `Ok(())` if the subtraction is successful.
	/// * `Err(anyhow::Error)` if the resulting bounding box is invalid.
	pub fn subtract(&mut self, x: u32, y: u32) {
		self.x_min = self.x_min.saturating_sub(x);
		self.y_min = self.y_min.saturating_sub(y);
		self.x_max = self.x_max.saturating_sub(x);
		self.y_max = self.y_max.saturating_sub(y);
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
		if scale == 0 {
			panic!("scale must be greater than 0");
		}

		self.x_min /= scale;
		self.y_min /= scale;
		self.x_max /= scale;
		self.y_max /= scale;
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
	/// An iterator yielding `TileCoord3` instances.
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord3> + '_ {
		let y_range = self.y_min..=self.y_max;
		let x_range = self.x_min..=self.x_max;
		y_range
			.cartesian_product(x_range)
			.map(|(y, x)| TileCoord3::new(x, y, self.level).unwrap())
	}

	/// Consumes the bounding box and returns an iterator over all tile coordinates within it.
	///
	/// The iteration is in row-major order.
	///
	/// # Returns
	///
	/// An iterator yielding `TileCoord3` instances.
	pub fn into_iter_coords(self) -> impl Iterator<Item = TileCoord3> {
		let y_range = self.y_min..=self.y_max;
		let x_range = self.x_min..=self.x_max;
		y_range
			.cartesian_product(x_range)
			.map(move |(y, x)| TileCoord3::new(x, y, self.level).unwrap())
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
		if size == 0 {
			return Box::new(std::iter::empty());
		}

		let level = self.level;
		let max = 2u32.pow(level as u32) - 1;
		let mut meta_bbox = *self;
		meta_bbox.scale_down(size);

		let iter = meta_bbox
			.iter_coords()
			.map(move |coord| {
				let x = coord.x * size;
				let y = coord.y * size;

				let mut bbox = TileBBox::new(level, x, y, (x + size - 1).min(max), (y + size - 1).min(max)).unwrap();
				bbox.intersect_bbox(self).unwrap();
				bbox
			})
			.filter(|bbox| !bbox.is_empty())
			.collect::<Vec<TileBBox>>()
			.into_iter();

		Box::new(iter)
	}

	// -------------------------------------------------------------------------
	// Utility Methods
	// -------------------------------------------------------------------------

	/// Retrieves the 0-based index of a `TileCoord2` within the bounding box.
	///
	/// # Arguments
	///
	/// * `coord` - Reference to the tile coordinate.
	///
	/// # Returns
	///
	/// * `Ok(usize)` representing the index if the coordinate is within the bounding box.
	/// * `Err(anyhow::Error)` if the coordinate is outside the bounding box.
	pub fn get_tile_index2(&self, coord: &TileCoord2) -> Result<usize> {
		if !self.contains2(coord) {
			return Err(anyhow::anyhow!(
				"Coordinate {:?} is not within the bounding box {:?}",
				coord,
				self
			));
		}

		let x = coord.x - self.x_min;
		let y = coord.y - self.y_min;
		let index = y * (self.x_max + 1 - self.x_min) + x;

		Ok(index as usize)
	}

	/// Retrieves the 0-based index of a `TileCoord3` within the bounding box.
	///
	/// # Arguments
	///
	/// * `coord` - Reference to the tile coordinate.
	///
	/// # Returns
	///
	/// * `Ok(usize)` representing the index if the coordinate is within the bounding box.
	/// * `Err(anyhow::Error)` if the coordinate is outside the bounding box or zoom levels do not match.
	pub fn get_tile_index3(&self, coord: &TileCoord3) -> Result<usize> {
		if !self.contains3(coord) {
			return Err(anyhow::anyhow!(
				"Coordinate {:?} is not within the bounding box {:?}",
				coord,
				self
			));
		}

		let x = coord.x - self.x_min;
		let y = coord.y - self.y_min;
		let index = y * (self.x_max + 1 - self.x_min) + x;

		Ok(index as usize)
	}

	/// Retrieves the `TileCoord2` at a specific index within the bounding box.
	///
	/// # Arguments
	///
	/// * `index` - 0-based index of the tile coordinate.
	///
	/// # Returns
	///
	/// * `Ok(TileCoord2)` if the index is within bounds.
	/// * `Err(anyhow::Error)` if the index is out of bounds.
	pub fn get_coord2_by_index(&self, index: u32) -> Result<TileCoord2> {
		ensure!(index < self.count_tiles() as u32, "index out of bounds");

		let width = self.width();
		Ok(TileCoord2::new(
			index.rem(width) + self.x_min,
			index.div(width) + self.y_min,
		))
	}

	/// Retrieves the `TileCoord3` at a specific index within the bounding box.
	///
	/// # Arguments
	///
	/// * `index` - 0-based index of the tile coordinate.
	///
	/// # Returns
	///
	/// * `Ok(TileCoord3)` if the index is within bounds.
	/// * `Err(anyhow::Error)` if the index is out of bounds.
	pub fn get_coord3_by_index(&self, index: u32) -> Result<TileCoord3> {
		ensure!(index < self.count_tiles() as u32, "index {index} out of bounds");

		let width = self.width();
		let x = index.rem(width) + self.x_min;
		let y = index.div(width) + self.y_min;
		TileCoord3::new(x, y, self.level)
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
			"{}: [{},{},{},{}] ({})",
			self.level,
			self.x_min,
			self.y_min,
			self.x_max,
			self.y_max,
			self.count_tiles()
		)
	}
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn count_tiles() {
		assert_eq!(TileBBox::new(4, 5, 12, 5, 12).unwrap().count_tiles(), 1);
		assert_eq!(TileBBox::new(4, 5, 12, 7, 15).unwrap().count_tiles(), 12);
		assert_eq!(TileBBox::new(4, 5, 12, 5, 15).unwrap().count_tiles(), 4);
		assert_eq!(TileBBox::new(4, 5, 15, 7, 15).unwrap().count_tiles(), 3);
	}

	#[test]
	fn from_geo() {
		let bbox1 = TileBBox::from_geo(9, &GeoBBox(8.0653, 51.3563, 12.3528, 52.2564)).unwrap();
		let bbox2 = TileBBox::new(9, 267, 168, 273, 170).unwrap();
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
		let geo_bbox2 = GeoBBox(0.0, -85.05112877980659f64, 180.0, 0.0);
		let mut geo_bbox0 = geo_bbox2;
		geo_bbox0.1 += 1e-10;
		geo_bbox0.2 -= 1e-10;
		for level in 1..32 {
			let level_bbox0 = TileBBox::from_geo(level, &geo_bbox0).unwrap();
			assert_eq!(level_bbox0.count_tiles(), 4u64.pow(level as u32 - 1));
			let geo_bbox1 = level_bbox0.as_geo_bbox();
			assert_eq!(geo_bbox1, geo_bbox2);
		}
	}

	#[test]
	fn sa_pacific() {
		let geo_bbox2 = GeoBBox(-180.0, -66.51326044311186f64, -90.0, 0.0);
		let mut geo_bbox0 = geo_bbox2;
		geo_bbox0.1 += 1e-10;
		geo_bbox0.2 -= 1e-10;

		for level in 2..32 {
			let level_bbox0 = TileBBox::from_geo(level, &geo_bbox0).unwrap();
			assert_eq!(level_bbox0.count_tiles(), 4u64.pow(level as u32 - 2));
			let geo_bbox1 = level_bbox0.as_geo_bbox();
			assert_eq!(geo_bbox1, geo_bbox2);
		}
	}

	#[test]
	fn get_tile_index() -> Result<()> {
		let bbox = TileBBox::new(8, 100, 100, 199, 199).unwrap();
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(100, 100))?, 0);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(101, 100))?, 1);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(199, 100))?, 99);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(100, 101))?, 100);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(100, 199))?, 9900);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(199, 199))?, 9999);
		Ok(())
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
		let bbox1 = TileBBox::new(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::new(4, 1, 10, 3, 12).unwrap();

		let mut bbox1_intersect = bbox1;
		bbox1_intersect.intersect_bbox(&bbox2).unwrap();
		assert_eq!(bbox1_intersect, TileBBox::new(4, 1, 11, 2, 12).unwrap());

		let mut bbox1_union = bbox1;
		bbox1_union.include_bbox(&bbox2).unwrap();
		assert_eq!(bbox1_union, TileBBox::new(4, 0, 10, 3, 13).unwrap());
	}

	#[test]
	fn include_tile() {
		let mut bbox = TileBBox::new(4, 0, 1, 2, 3).unwrap();
		bbox.include_coord(4, 5);
		assert_eq!(bbox, TileBBox::new(4, 0, 1, 4, 5).unwrap());
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
		let bbox = TileBBox::new(16, 1, 5, 2, 6).unwrap();
		let vec: Vec<TileCoord3> = bbox.iter_coords().collect();
		assert_eq!(vec.len(), 4);
		assert_eq!(vec[0], TileCoord3::new(1, 5, 16).unwrap());
		assert_eq!(vec[1], TileCoord3::new(2, 5, 16).unwrap());
		assert_eq!(vec[2], TileCoord3::new(1, 6, 16).unwrap());
		assert_eq!(vec[3], TileCoord3::new(2, 6, 16).unwrap());
	}

	#[test]
	fn iter_bbox_grid() {
		fn b(level: u8, x_min: u32, y_min: u32, x_max: u32, y_max: u32) -> TileBBox {
			TileBBox::new(level, x_min, y_min, x_max, y_max).unwrap()
		}
		fn test(size: u32, bbox: TileBBox, bboxes: &str) {
			let bboxes_result: String = bbox
				.iter_bbox_grid(size)
				.map(|bbox| format!("{},{},{},{}", bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max))
				.collect::<Vec<String>>()
				.join(" ");
			assert_eq!(bboxes_result, bboxes);
		}

		test(16, b(10, 0, 0, 31, 31), "0,0,15,15 16,0,31,15 0,16,15,31 16,16,31,31");
		test(16, b(10, 5, 6, 25, 26), "5,6,15,15 16,6,25,15 5,16,15,26 16,16,25,26");
		test(16, b(10, 5, 6, 16, 16), "5,6,15,15 16,6,16,15 5,16,15,16 16,16,16,16");
		test(16, b(10, 5, 6, 16, 15), "5,6,15,15 16,6,16,15");
		test(16, b(10, 6, 7, 6, 7), "6,7,6,7");
		test(64, b(4, 6, 7, 6, 7), "6,7,6,7");
		test(16, TileBBox::new_empty(10).unwrap(), "");
	}

	#[test]
	fn add_border() {
		let mut bbox = TileBBox::new(8, 5, 10, 20, 30).unwrap();

		// Border of (1, 1, 1, 1) should increase the size of the bbox by 1 in all directions
		bbox.add_border(1, 1, 1, 1);
		assert_eq!(bbox, TileBBox::new(8, 4, 9, 21, 31).unwrap());

		// Border of (2, 3, 4, 5) should further increase the size of the bbox
		bbox.add_border(2, 3, 4, 5);
		assert_eq!(bbox, TileBBox::new(8, 2, 6, 25, 36).unwrap());

		// Border of (0, 0, 0, 0) should not change the size of the bbox
		bbox.add_border(0, 0, 0, 0);
		assert_eq!(bbox, TileBBox::new(8, 2, 6, 25, 36).unwrap());

		// Large border should saturate at max=255 for level=8
		bbox.add_border(999, 999, 999, 999);
		assert_eq!(bbox, TileBBox::new(8, 0, 0, 255, 255).unwrap());

		// If bbox is empty, add_border should have no effect
		let mut empty_bbox = TileBBox::new_empty(8).unwrap();
		empty_bbox.add_border(1, 2, 3, 4);
		assert_eq!(empty_bbox, TileBBox::new_empty(8).unwrap());
	}

	#[test]
	fn test_shift_by() {
		let mut bbox = TileBBox::new(4, 1, 2, 3, 4).unwrap();
		bbox.shift_by(1, 1);
		assert_eq!(bbox, TileBBox::new(4, 2, 3, 4, 5).unwrap());
	}

	#[test]
	fn test_get_max() {
		let bbox = TileBBox::new(4, 1, 1, 3, 3).unwrap();
		assert_eq!(bbox.max, 15);
	}

	#[test]
	fn test_new_empty() {
		let bbox = TileBBox::new_empty(4).unwrap();
		assert_eq!(
			bbox,
			TileBBox {
				level: 4,
				max: 15,
				x_min: 16,
				y_min: 16,
				x_max: 0,
				y_max: 0,
			}
		);
		assert!(bbox.is_empty());
	}

	#[test]
	fn test_set_empty() {
		let mut bbox = TileBBox::new(4, 0, 0, 15, 15).unwrap();
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
		assert!(bbox.is_full());
	}

	#[test]
	fn test_include_tile() {
		let mut bbox = TileBBox::new(6, 5, 10, 20, 30).unwrap();
		bbox.include_coord(25, 35);
		assert_eq!(bbox, TileBBox::new(6, 5, 10, 25, 35).unwrap());
	}

	#[test]
	fn test_include_bbox() {
		let mut bbox1 = TileBBox::new(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::new(4, 1, 10, 3, 12).unwrap();
		bbox1.include_bbox(&bbox2).unwrap();
		assert_eq!(bbox1, TileBBox::new(4, 0, 10, 3, 13).unwrap());
	}

	#[test]
	fn test_intersect_bbox() {
		let mut bbox1 = TileBBox::new(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::new(4, 1, 10, 3, 12).unwrap();
		bbox1.intersect_bbox(&bbox2).unwrap();
		assert_eq!(bbox1, TileBBox::new(4, 1, 11, 2, 12).unwrap());
	}

	#[test]
	fn test_overlaps_bbox() {
		let bbox1 = TileBBox::new(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::new(4, 1, 10, 3, 12).unwrap();
		assert!(bbox1.overlaps_bbox(&bbox2).unwrap());

		let bbox3 = TileBBox::new(4, 8, 8, 9, 9).unwrap();
		assert!(!bbox1.overlaps_bbox(&bbox3).unwrap());
	}

	#[test]
	fn test_get_tile_index2() -> Result<()> {
		let bbox = TileBBox::new(8, 100, 100, 199, 199).unwrap();
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(100, 100))?, 0);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(101, 100))?, 1);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(199, 100))?, 99);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(100, 101))?, 100);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(100, 199))?, 9900);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(199, 199))?, 9999);
		Ok(())
	}

	#[test]
	fn test_get_tile_index3() -> Result<()> {
		let bbox = TileBBox::new(8, 100, 100, 199, 199).unwrap();
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(100, 100, 8)?)?, 0);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(101, 100, 8)?)?, 1);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(199, 100, 8)?)?, 99);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(100, 101, 8)?)?, 100);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(100, 199, 8)?)?, 9900);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(199, 199, 8)?)?, 9999);
		Ok(())
	}

	#[test]
	fn test_get_coord2_by_index() {
		let bbox = TileBBox::new(4, 5, 10, 7, 12).unwrap();
		assert_eq!(bbox.get_coord2_by_index(0).unwrap(), TileCoord2::new(5, 10));
		assert_eq!(bbox.get_coord2_by_index(1).unwrap(), TileCoord2::new(6, 10));
		assert_eq!(bbox.get_coord2_by_index(2).unwrap(), TileCoord2::new(7, 10));
		assert_eq!(bbox.get_coord2_by_index(3).unwrap(), TileCoord2::new(5, 11));
		assert_eq!(bbox.get_coord2_by_index(8).unwrap(), TileCoord2::new(7, 12));
	}

	#[test]
	fn test_get_coord3_by_index() {
		let bbox = TileBBox::new(4, 5, 10, 7, 12).unwrap();
		assert_eq!(bbox.get_coord3_by_index(0).unwrap(), TileCoord3::new(5, 10, 4).unwrap());
		assert_eq!(bbox.get_coord3_by_index(1).unwrap(), TileCoord3::new(6, 10, 4).unwrap());
		assert_eq!(bbox.get_coord3_by_index(2).unwrap(), TileCoord3::new(7, 10, 4).unwrap());
		assert_eq!(bbox.get_coord3_by_index(3).unwrap(), TileCoord3::new(5, 11, 4).unwrap());
		assert_eq!(bbox.get_coord3_by_index(8).unwrap(), TileCoord3::new(7, 12, 4).unwrap());
	}

	#[test]
	fn test_as_geo_bbox() {
		let bbox = TileBBox::new(4, 5, 10, 7, 12).unwrap();
		let geo_bbox = bbox.as_geo_bbox();
		assert_eq!(
			geo_bbox.as_string_list(),
			"-67.5,-74.01954331150228,0,-40.97989806962013"
		);
	}

	#[test]
	fn test_contains2() {
		let bbox = TileBBox::new(4, 5, 10, 7, 12).unwrap();
		assert!(bbox.contains2(&TileCoord2::new(6, 11)));
		assert!(!bbox.contains2(&TileCoord2::new(4, 9)));
	}

	#[test]
	fn test_contains3() {
		let bbox = TileBBox::new(4, 5, 10, 7, 12).unwrap();
		assert!(bbox.contains3(&TileCoord3::new(6, 11, 4).unwrap()));
		assert!(!bbox.contains3(&TileCoord3::new(4, 9, 4).unwrap()));
		assert!(!bbox.contains3(&TileCoord3::new(6, 11, 5).unwrap()));
	}

	#[test]
	fn test_new_valid_bbox() {
		let bbox = TileBBox::new(6, 5, 10, 15, 20).unwrap();
		assert_eq!(bbox.level, 6);
		assert_eq!(bbox.x_min, 5);
		assert_eq!(bbox.y_min, 10);
		assert_eq!(bbox.x_max, 15);
		assert_eq!(bbox.y_max, 20);
		assert_eq!(bbox.max, 63);
	}

	#[test]
	fn test_new_invalid_level() {
		let result = TileBBox::new(32, 0, 0, 1, 1);
		assert!(result.is_err());
	}

	#[test]
	fn test_new_invalid_coordinates() {
		let result = TileBBox::new(4, 10, 10, 5, 15);
		assert!(result.is_err());

		let result = TileBBox::new(4, 5, 15, 7, 10);
		assert!(result.is_err());

		let result = TileBBox::new(4, 0, 0, 16, 15); // x_max exceeds max for level 4
		assert!(result.is_err());
	}

	#[test]
	fn test_new_full() {
		let bbox = TileBBox::new_full(4).unwrap();
		assert_eq!(bbox, TileBBox::new(4, 0, 0, 15, 15).unwrap());
		assert!(bbox.is_full());
	}

	#[test]
	fn test_from_geo_valid() {
		let geo_bbox = GeoBBox(-180.0, -85.05112878, 180.0, 85.05112878);
		let bbox = TileBBox::from_geo(2, &geo_bbox).unwrap();
		assert_eq!(bbox, TileBBox::new(2, 0, 0, 3, 3).unwrap());
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

		let non_empty_bbox = TileBBox::new(6, 5, 10, 15, 20).unwrap();
		assert!(!non_empty_bbox.is_empty());
	}

	#[test]
	fn test_width_height() {
		let bbox = TileBBox::new(6, 5, 10, 15, 20).unwrap();
		assert_eq!(bbox.width(), 11);
		assert_eq!(bbox.height(), 11);

		let empty_bbox = TileBBox::new_empty(4).unwrap();
		assert_eq!(empty_bbox.width(), 0);
		assert_eq!(empty_bbox.height(), 0);
	}

	#[test]
	fn test_count_tiles() {
		let bbox = TileBBox::new(6, 5, 10, 15, 20).unwrap();
		assert_eq!(bbox.count_tiles(), 121);

		let empty_bbox = TileBBox::new_empty(4).unwrap();
		assert_eq!(empty_bbox.count_tiles(), 0);
	}

	#[test]
	fn test_include_coord() -> Result<()> {
		let mut bbox = TileBBox::new_empty(6)?;
		bbox.include_coord(5, 10);
		assert_eq!(bbox, TileBBox::new(6, 5, 10, 5, 10).unwrap());

		bbox.include_coord(15, 20);
		assert_eq!(bbox, TileBBox::new(6, 5, 10, 15, 20).unwrap());

		bbox.include_coord(10, 15);
		assert_eq!(bbox, TileBBox::new(6, 5, 10, 15, 20).unwrap());

		Ok(())
	}

	#[test]
	fn test_include_coord3() -> Result<()> {
		let mut bbox = TileBBox::new_empty(6)?;
		let coord = TileCoord3::new(5, 10, 6).unwrap();
		bbox.include_coord3(&coord)?;
		assert_eq!(bbox, TileBBox::new(6, 5, 10, 5, 10).unwrap());

		let coord = TileCoord3::new(15, 20, 6).unwrap();
		bbox.include_coord3(&coord)?;
		assert_eq!(bbox, TileBBox::new(6, 5, 10, 15, 20).unwrap());

		// Attempt to include a coordinate with a different zoom level
		let coord_invalid = TileCoord3::new(10, 15, 5).unwrap();
		let result = bbox.include_coord3(&coord_invalid);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn test_add_border() -> Result<()> {
		let mut bbox = TileBBox::new(6, 5, 10, 15, 20)?;

		// Add a border within bounds
		bbox.add_border(2, 3, 2, 3);
		assert_eq!(bbox, TileBBox::new(6, 3, 7, 17, 23).unwrap());

		// Add a border that exceeds bounds, should clamp to max
		bbox.add_border(10, 10, 10, 10);
		assert_eq!(bbox, TileBBox::new(6, 0, 0, 27, 33).unwrap());

		// Add border to an empty bounding box, should have no effect
		let mut empty_bbox = TileBBox::new_empty(6)?;
		empty_bbox.add_border(1, 1, 1, 1);
		assert!(empty_bbox.is_empty());

		// Attempt to add a border with zero values
		bbox.add_border(0, 0, 0, 0);
		assert_eq!(bbox, TileBBox::new(6, 0, 0, 27, 33).unwrap());

		Ok(())
	}

	#[test]
	fn should_include_bbox_correctly_with_valid_and_empty_bboxes() -> Result<()> {
		let mut bbox1 = TileBBox::new(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::new(6, 10, 15, 20, 25)?;

		bbox1.include_bbox(&bbox2)?;
		assert_eq!(bbox1, TileBBox::new(6, 5, 10, 20, 25).unwrap());

		// Including an empty bounding box should have no effect
		let empty_bbox = TileBBox::new_empty(6)?;
		bbox1.include_bbox(&empty_bbox)?;
		assert_eq!(bbox1, TileBBox::new(6, 5, 10, 20, 25).unwrap());

		// Attempting to include a bounding box with different zoom level
		let bbox_diff_level = TileBBox::new(5, 5, 10, 20, 25)?;
		let result = bbox1.include_bbox(&bbox_diff_level);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_intersect_bboxes_correctly_and_handle_empty_and_different_levels() -> Result<()> {
		let mut bbox1 = TileBBox::new(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::new(6, 10, 15, 20, 25)?;

		bbox1.intersect_bbox(&bbox2)?;
		assert_eq!(bbox1, TileBBox::new(6, 10, 15, 15, 20).unwrap());

		// Intersect with a non-overlapping bounding box
		let bbox3 = TileBBox::new(6, 16, 21, 20, 25)?;
		bbox1.intersect_bbox(&bbox3)?;
		assert!(bbox1.is_empty());

		// Attempting to intersect with a bounding box of different zoom level
		let bbox_diff_level = TileBBox::new(5, 10, 15, 15, 20)?;
		let result = bbox1.intersect_bbox(&bbox_diff_level);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_correctly_determine_bbox_overlap() -> Result<()> {
		let bbox1 = TileBBox::new(6, 5, 10, 15, 20)?;
		let bbox2 = TileBBox::new(6, 10, 15, 20, 25)?;
		let bbox3 = TileBBox::new(6, 16, 21, 20, 25)?;
		let bbox4 = TileBBox::new(5, 10, 15, 15, 20)?;

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
	fn should_get_correct_tile_index2() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 7, 12)?;

		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(5, 10)).unwrap(), 0);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(6, 10)).unwrap(), 1);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(7, 10)).unwrap(), 2);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(5, 11)).unwrap(), 3);
		assert_eq!(bbox.get_tile_index2(&TileCoord2::new(6, 12)).unwrap(), 7);

		// Attempt to get index of a coordinate outside the bounding box
		let result = bbox.get_tile_index2(&TileCoord2::new(4, 9));
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_get_correct_tile_index3() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 7, 12)?;

		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(5, 10, 4).unwrap()).unwrap(), 0);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(6, 10, 4).unwrap()).unwrap(), 1);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(7, 10, 4).unwrap()).unwrap(), 2);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(5, 11, 4).unwrap()).unwrap(), 3);
		assert_eq!(bbox.get_tile_index3(&TileCoord3::new(7, 12, 4).unwrap()).unwrap(), 8);

		// Attempt to get index of a coordinate outside the bounding box
		let coord_outside = TileCoord3::new(4, 9, 4).unwrap();
		let result = bbox.get_tile_index3(&coord_outside);
		assert!(result.is_err());

		// Attempt to get index with mismatched zoom level
		let coord_diff_level = TileCoord3::new(5, 10, 5).unwrap();
		let result = bbox.get_tile_index3(&coord_diff_level);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_get_coord2_by_index_correctly() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 7, 12)?;

		assert_eq!(bbox.get_coord2_by_index(0).unwrap(), TileCoord2::new(5, 10));
		assert_eq!(bbox.get_coord2_by_index(1).unwrap(), TileCoord2::new(6, 10));
		assert_eq!(bbox.get_coord2_by_index(2).unwrap(), TileCoord2::new(7, 10));
		assert_eq!(bbox.get_coord2_by_index(3).unwrap(), TileCoord2::new(5, 11));
		assert_eq!(bbox.get_coord2_by_index(8).unwrap(), TileCoord2::new(7, 12));

		// Attempt to get coordinate with out-of-bounds index
		let result = bbox.get_coord2_by_index(9);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_get_coord3_by_index_correctly() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 7, 12)?;

		assert_eq!(bbox.get_coord3_by_index(0).unwrap(), TileCoord3::new(5, 10, 4).unwrap());
		assert_eq!(bbox.get_coord3_by_index(1).unwrap(), TileCoord3::new(6, 10, 4).unwrap());
		assert_eq!(bbox.get_coord3_by_index(2).unwrap(), TileCoord3::new(7, 10, 4).unwrap());
		assert_eq!(bbox.get_coord3_by_index(3).unwrap(), TileCoord3::new(5, 11, 4).unwrap());
		assert_eq!(bbox.get_coord3_by_index(8).unwrap(), TileCoord3::new(7, 12, 4).unwrap());

		// Attempt to get coordinate with out-of-bounds index
		let result = bbox.get_coord3_by_index(9);
		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn should_convert_to_geo_bbox_correctly() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 7, 12)?;
		let geo_bbox = bbox.as_geo_bbox();

		// Assuming TileCoord3::as_geo() converts tile coordinates to geographical coordinates correctly,
		// the following is an example expected output. Adjust based on actual implementation.
		// For demonstration, let's assume:
		// - Tile (5, 10, 4) maps to longitude -67.5 and latitude 74.01954331
		// - Tile (7, 12, 4) maps to longitude 0.0 and latitude 40.97989807
		let expected_geo_bbox = GeoBBox(-67.5, -74.01954331150228, 0.0, -40.97989806962013);
		assert_eq!(geo_bbox, expected_geo_bbox);

		Ok(())
	}

	#[test]
	fn should_determine_contains2_correctly() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 7, 12)?;

		assert!(bbox.contains2(&TileCoord2::new(5, 10)));
		assert!(bbox.contains2(&TileCoord2::new(6, 11)));
		assert!(bbox.contains2(&TileCoord2::new(7, 12)));
		assert!(!bbox.contains2(&TileCoord2::new(4, 9)));
		assert!(!bbox.contains2(&TileCoord2::new(8, 13)));

		Ok(())
	}

	#[test]
	fn should_determine_contains3_correctly() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 7, 12)?;
		let valid_coord = TileCoord3::new(6, 11, 4).unwrap();
		let invalid_coord_zoom = TileCoord3::new(6, 11, 5).unwrap();
		let invalid_coord_outside = TileCoord3::new(4, 9, 4).unwrap();

		assert!(bbox.contains3(&valid_coord));
		assert!(!bbox.contains3(&invalid_coord_zoom));
		assert!(!bbox.contains3(&invalid_coord_outside));

		Ok(())
	}

	#[test]
	fn should_iterate_over_coords_correctly() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 6, 11)?;
		let coords: Vec<TileCoord3> = bbox.iter_coords().collect();
		let expected_coords = vec![
			TileCoord3::new(5, 10, 4).unwrap(),
			TileCoord3::new(6, 10, 4).unwrap(),
			TileCoord3::new(5, 11, 4).unwrap(),
			TileCoord3::new(6, 11, 4).unwrap(),
		];
		assert_eq!(coords, expected_coords);

		Ok(())
	}

	#[test]
	fn should_iterate_over_coords_correctly_when_consumed() -> Result<()> {
		let bbox = TileBBox::new(4, 5, 10, 6, 11)?;
		let coords: Vec<TileCoord3> = bbox.into_iter_coords().collect();
		let expected_coords = vec![
			TileCoord3::new(5, 10, 4).unwrap(),
			TileCoord3::new(6, 10, 4).unwrap(),
			TileCoord3::new(5, 11, 4).unwrap(),
			TileCoord3::new(6, 11, 4).unwrap(),
		];
		assert_eq!(coords, expected_coords);

		Ok(())
	}

	#[test]
	fn should_split_bbox_into_correct_grid() -> Result<()> {
		let bbox = TileBBox::new(4, 0, 0, 7, 7)?;

		let grid_size = 4;
		let grids: Vec<TileBBox> = bbox.iter_bbox_grid(grid_size).collect();

		let expected_grids = vec![
			TileBBox::new(4, 0, 0, 3, 3)?,
			TileBBox::new(4, 4, 0, 7, 3)?,
			TileBBox::new(4, 0, 4, 3, 7)?,
			TileBBox::new(4, 4, 4, 7, 7)?,
		];

		assert_eq!(grids, expected_grids);

		Ok(())
	}

	#[test]
	fn should_scale_down_correctly() -> Result<()> {
		let mut bbox = TileBBox::new(4, 4, 4, 7, 7)?;
		bbox.scale_down(2);
		assert_eq!(bbox, TileBBox::new(4, 2, 2, 3, 3)?);

		// Scaling down by a factor larger than the coordinates
		bbox.scale_down(5);
		assert_eq!(bbox, TileBBox::new(4, 0, 0, 0, 0)?);

		Ok(())
	}

	#[test]
	fn should_shift_bbox_correctly() -> Result<()> {
		let mut bbox = TileBBox::new(6, 5, 10, 15, 20)?;
		bbox.shift_by(3, 4);
		assert_eq!(bbox, TileBBox::new(6, 8, 14, 18, 24)?);

		// Shifting beyond max should not cause overflow due to saturating_add
		let mut bbox = TileBBox::new(6, 14, 14, 15, 15)?;
		bbox.shift_by(2, 2);
		assert_eq!(bbox, TileBBox::new(6, 16, 16, 17, 17)?);

		Ok(())
	}

	#[test]
	fn should_subtract_coord2_correctly() -> Result<()> {
		let mut bbox = TileBBox::new(6, 5, 10, 15, 20)?;
		let coord = TileCoord2::new(3, 5);
		bbox.subtract_coord2(&coord);
		assert_eq!(bbox, TileBBox::new(6, 2, 5, 12, 15)?);

		// Subtracting more than current coordinates should saturate at 0
		bbox.subtract_coord2(&TileCoord2::new(5, 10));
		assert_eq!(bbox, TileBBox::new(6, 0, 0, 7, 5)?);

		Ok(())
	}

	#[test]
	fn should_subtract_u32_correctly() -> Result<()> {
		let mut bbox = TileBBox::new(6, 5, 10, 15, 20)?;
		bbox.subtract(3, 5);
		assert_eq!(bbox, TileBBox::new(6, 2, 5, 12, 15)?);

		// Subtracting more than current coordinates should saturate at 0
		bbox.subtract(5, 10);
		assert_eq!(bbox, TileBBox::new(6, 0, 0, 7, 5)?);

		Ok(())
	}

	#[test]
	fn should_handle_bbox_overlap_edge_cases() -> Result<()> {
		let bbox1 = TileBBox::new(4, 0, 0, 5, 5)?;
		let bbox2 = TileBBox::new(4, 5, 5, 10, 10)?;
		let bbox3 = TileBBox::new(4, 6, 6, 10, 10)?;
		let bbox4 = TileBBox::new(4, 0, 0, 5, 5)?;

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
		let bbox = TileBBox::new(4, 5, 10, 5, 10)?;
		let grids: Vec<TileBBox> = bbox.iter_bbox_grid(4).collect();
		let expected_grids = vec![TileBBox::new(4, 5, 10, 5, 10).unwrap()];
		assert_eq!(grids, expected_grids);
		Ok(())
	}
}
