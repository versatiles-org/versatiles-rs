//! This module defines the `TileBBox` struct, which represents a bounding box for tiles at a specific zoom level.
//! It provides methods to create, manipulate, and query these bounding boxes.

use super::{TileCoord2, TileCoord3};
use anyhow::{ensure, Result};
use itertools::Itertools;
use std::{
	fmt,
	ops::{Div, Rem},
};

/// A struct that represents a bounding box for tiles at a specific zoom level.
#[derive(Clone, PartialEq, Eq)]
pub struct TileBBox {
	/// The zoom level of the bounding box.
	pub level: u8,
	/// The minimum x-coordinate of the bounding box.
	pub x_min: u32,
	/// The minimum y-coordinate of the bounding box.
	pub y_min: u32,
	/// The maximum x-coordinate of the bounding box.
	pub x_max: u32,
	/// The maximum y-coordinate of the bounding box.
	pub y_max: u32,
	/// The maximum coordinate value at the current zoom level.
	pub max: u32,
}

#[allow(dead_code)]
impl TileBBox {
	/// Creates a new `TileBBox` with the specified coordinates and zoom level.
	///
	/// # Arguments
	///
	/// * `level` - The zoom level of the bounding box.
	/// * `x_min` - The minimum x-coordinate.
	/// * `y_min` - The minimum y-coordinate.
	/// * `x_max` - The maximum x-coordinate.
	/// * `y_max` - The maximum y-coordinate.
	///
	/// # Returns
	///
	/// A `Result` containing the new `TileBBox` or an error if the coordinates are invalid.
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

		bbox.check()?;

		Ok(bbox)
	}

	/// Creates a new `TileBBox` that covers the entire range of tiles at the specified zoom level.
	///
	/// # Arguments
	///
	/// * `level` - The zoom level of the bounding box.
	///
	/// # Returns
	///
	/// A `Result` containing the new `TileBBox` or an error if the zoom level is invalid.
	pub fn new_full(level: u8) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		let max = 2u32.pow(level as u32) - 1;
		TileBBox::new(level, 0, 0, max, max)
	}

	/// Creates a new empty `TileBBox` at the specified zoom level.
	///
	/// # Arguments
	///
	/// * `level` - The zoom level of the bounding box.
	///
	/// # Returns
	///
	/// A `Result` containing the new empty `TileBBox` or an error if the zoom level is invalid.
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

	/// Creates a new `TileBBox` from geographical coordinates.
	///
	/// # Arguments
	///
	/// * `level` - The zoom level of the bounding box.
	/// * `geo_bbox` - A reference to an array of four `f64` values representing the geographical bounding box.
	///
	/// # Returns
	///
	/// A `Result` containing the new `TileBBox` or an error if the coordinates are invalid.
	pub fn from_geo(level: u8, geo_bbox: &[f64; 4]) -> Result<TileBBox> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		ensure!(geo_bbox[0] >= -180., "x_min ({}) must be >= -180", geo_bbox[0]);
		ensure!(geo_bbox[1] >= -90., "y_min ({}) must be >= -90", geo_bbox[1]);
		ensure!(geo_bbox[2] <= 180., "x_max ({}) must be <= 180", geo_bbox[2]);
		ensure!(geo_bbox[3] <= 90., "y_max ({}) must be <= 90", geo_bbox[3]);
		ensure!(
			geo_bbox[0] <= geo_bbox[2],
			"x_min ({}) must be <= x_max ({})",
			geo_bbox[0],
			geo_bbox[2]
		);
		ensure!(
			geo_bbox[1] <= geo_bbox[3],
			"y_min ({}) must be <= y_max ({})",
			geo_bbox[1],
			geo_bbox[3]
		);

		let p_min = TileCoord2::from_geo(geo_bbox[0], geo_bbox[3], level, false)?;
		let p_max = TileCoord2::from_geo(geo_bbox[2], geo_bbox[1], level, true)?;

		TileBBox::new(level, p_min.get_x(), p_min.get_y(), p_max.get_x(), p_max.get_y())
	}

	/// Sets the bounding box to an empty state.
	pub fn set_empty(&mut self) {
		self.x_min = 1;
		self.y_min = 1;
		self.x_max = 0;
		self.y_max = 0;
	}

	/// Checks if the bounding box is empty.
	///
	/// # Returns
	///
	/// `true` if the bounding box is empty, `false` otherwise.
	pub fn is_empty(&self) -> bool {
		(self.x_max < self.x_min) || (self.y_max < self.y_min)
	}

	/// Sets the bounding box to a full state. (Test only)
	#[cfg(test)]
	pub fn set_full(&mut self) {
		self.x_min = 0;
		self.y_min = 0;
		self.x_max = self.max;
		self.y_max = self.max;
	}

	/// Checks if the bounding box is full. (Test only)
	#[cfg(test)]
	pub fn is_full(&self) -> bool {
		(self.x_min == 0) && (self.y_min == 0) && (self.x_max == self.max) && (self.y_max == self.max)
	}

	/// Counts the number of tiles within the bounding box.
	///
	/// # Returns
	///
	/// The number of tiles within the bounding box.
	pub fn count_tiles(&self) -> u64 {
		if self.x_max < self.x_min {
			return 0;
		}
		if self.y_max < self.y_min {
			return 0;
		}

		(self.x_max - self.x_min + 1) as u64 * (self.y_max - self.y_min + 1) as u64
	}

	/// Includes a tile coordinate within the bounding box.
	///
	/// # Arguments
	///
	/// * `x` - The x-coordinate of the tile.
	/// * `y` - The y-coordinate of the tile.
	pub fn include_tile(&mut self, x: u32, y: u32) {
		if self.is_empty() {
			self.x_min = x;
			self.y_min = y;
			self.x_max = x;
			self.y_max = y;
		} else {
			self.x_min = self.x_min.min(x);
			self.y_min = self.y_min.min(y);
			self.x_max = self.x_max.max(x).min(self.max);
			self.y_max = self.y_max.max(y).min(self.max);
		}
	}

	/// Expands the bounding box to include another bounding box.
	///
	/// # Arguments
	///
	/// * `bbox` - A reference to the bounding box to include.
	pub fn union_bbox(&mut self, bbox: &TileBBox) {
		if !bbox.is_empty() {
			if self.is_empty() {
				self.set_bbox(bbox);
			} else {
				self.x_min = self.x_min.min(bbox.x_min);
				self.y_min = self.y_min.min(bbox.y_min);
				self.x_max = self.x_max.max(bbox.x_max).min(self.max);
				self.y_max = self.y_max.max(bbox.y_max).min(self.max);
			}
		}
	}

	/// Intersects the bounding box with another bounding box.
	///
	/// # Arguments
	///
	/// * `bbox` - A reference to the bounding box to intersect with.
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) {
		if !self.is_empty() {
			self.x_min = self.x_min.max(bbox.x_min);
			self.y_min = self.y_min.max(bbox.y_min);
			self.x_max = self.x_max.min(bbox.x_max);
			self.y_max = self.y_max.min(bbox.y_max);
		}
	}

	/// Adds a border to the bounding box.
	///
	/// # Arguments
	///
	/// * `x_min` - The amount to subtract from the minimum x-coordinate.
	/// * `y_min` - The amount to subtract from the minimum y-coordinate.
	/// * `x_max` - The amount to add to the maximum x-coordinate.
	/// * `y_max` - The amount to add to the maximum y-coordinate.
	pub fn add_border(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) {
		if !self.is_empty() {
			self.x_min -= self.x_min.min(x_min);
			self.y_min -= self.y_min.min(y_min);
			self.x_max = (self.x_max + x_max).min(self.max);
			self.y_max = (self.y_max + y_max).min(self.max);
		}
	}

	/// Sets the bounding box to the specified bounding box.
	///
	/// # Arguments
	///
	/// * `bbox` - A reference to the bounding box to set.
	pub fn set_bbox(&mut self, bbox: &TileBBox) {
		self.x_min = bbox.x_min;
		self.y_min = bbox.y_min;
		self.x_max = bbox.x_max;
		self.y_max = bbox.y_max;
	}

	/// Returns an iterator over the tile coordinates within the bounding box.
	///
	/// # Returns
	///
	/// An iterator over the tile coordinates.
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord3> + '_ {
		let y_range = self.y_min..=self.y_max;
		let x_range = self.x_min..=self.x_max;
		y_range
			.cartesian_product(x_range)
			.map(|(y, x)| TileCoord3::new(x, y, self.level).unwrap())
	}

	/// Splits the bounding box into a grid of bounding boxes of the specified size.
	///
	/// # Arguments
	///
	/// * `size` - The size of the grid.
	///
	/// # Returns
	///
	/// An iterator over the grid of bounding boxes.
	pub fn iter_bbox_grid(&self, size: u32) -> impl Iterator<Item = TileBBox> + '_ {
		let level = self.level;
		let max = 2u32.pow(level as u32) - 1;
		let mut meta_bbox = self.clone();
		meta_bbox.scale_down(size);

		meta_bbox
			.iter_coords()
			.map(move |coord| {
				let x = coord.x * size;
				let y = coord.y * size;

				let mut bbox = TileBBox::new(level, x, y, (x + size - 1).min(max), (y + size - 1).min(max)).unwrap();
				bbox.intersect_bbox(self);
				bbox
			})
			.filter(|bbox| !bbox.is_empty())
			.collect::<Vec<TileBBox>>()
			.into_iter()
	}

	/// Splits the bounding box into row slices with a maximum number of elements.
	///
	/// # Arguments
	///
	/// * `max_count` - The maximum number of elements in each slice.
	///
	/// # Returns
	///
	/// An iterator over the row slices.
	pub fn iter_bbox_row_slices(&self, max_count: usize) -> impl Iterator<Item = TileBBox> + '_ {
		let mut col_count = (self.x_max - self.x_min + 1) as usize;
		let mut row_count = (self.y_max - self.y_min + 1) as usize;

		let mut col_pos: Vec<u32> = Vec::new();
		let mut row_pos: Vec<u32> = Vec::new();

		if max_count <= col_count {
			// split each row into chunks

			let col_chunk_count = (col_count as f64 / max_count as f64).ceil() as usize;
			let col_chunk_size = col_count as f64 / col_chunk_count as f64;
			for col in 0..=col_chunk_count {
				col_pos.insert(col, (col_chunk_size * col as f64) as u32 + self.x_min)
			}
			col_count = col_chunk_count;

			for row in self.y_min..=self.y_max + 1 {
				row_pos.insert((row - self.y_min) as usize, row)
			}
		} else {
			// each chunk consists of multiple rows

			let row_chunk_max_size = max_count / col_count;
			let row_chunk_count = (row_count as f64 / row_chunk_max_size as f64).ceil() as usize;
			let row_chunk_size = row_count as f64 / row_chunk_count as f64;
			for row in 0..=row_chunk_count {
				row_pos.insert(row, (row_chunk_size * row as f64).round() as u32 + self.y_min)
			}
			row_count = row_chunk_count;

			col_pos.insert(0, self.x_min);
			col_pos.insert(1, self.x_max + 1);
			col_count = 1;
		}

		assert_eq!(col_pos[0], self.x_min, "incorrect x_min");
		assert_eq!(row_pos[0], self.y_min, "incorrect y_min");
		assert_eq!(col_pos[col_count] - 1, self.x_max, "incorrect x_max");
		assert_eq!(row_pos[row_count] - 1, self.y_max, "incorrect y_max");

		let cols = 0..col_count;
		let rows = 0..row_count;

		rows.cartesian_product(cols).map(move |(row, col)| {
			TileBBox::new(
				self.level,
				col_pos[col],
				row_pos[row],
				col_pos[col + 1] - 1,
				row_pos[row + 1] - 1,
			)
			.unwrap()
		})
	}

	/// Checks the validity of the bounding box.
	///
	/// # Returns
	///
	/// A `Result` indicating success or failure.
	fn check(&self) -> Result<()> {
		ensure!(
			self.x_min <= self.x_max,
			"x_min ({}) must be <= x_max ({})",
			self.x_min,
			self.x_max
		);
		ensure!(
			self.y_min <= self.y_max,
			"y_min ({}) must be <= y_max ({})",
			self.y_min,
			self.y_max
		);
		ensure!(
			self.x_max <= self.max,
			"x_max ({}) must be <= max ({})",
			self.x_max,
			self.max
		);
		ensure!(
			self.y_max <= self.max,
			"y_max ({}) must be <= max ({})",
			self.y_max,
			self.max
		);
		Ok(())
	}

	/// Shifts the bounding box by the specified coordinates.
	///
	/// # Arguments
	///
	/// * `x` - The amount to shift the x-coordinates.
	/// * `y` - The amount to shift the y-coordinates.
	pub fn shift_by(&mut self, x: u32, y: u32) {
		self.x_min += x;
		self.y_min += y;
		self.x_max += x;
		self.y_max += y;

		self.check().unwrap();
	}

	/// Subtracts the coordinates from the bounding box.
	///
	/// # Arguments
	///
	/// * `c` - A reference to the coordinates to subtract.
	pub fn substract_coord2(&mut self, c: &TileCoord2) {
		self.x_min = self.x_min.saturating_sub(c.get_x());
		self.y_min = self.y_min.saturating_sub(c.get_y());
		self.x_max = self.x_max.saturating_sub(c.get_x());
		self.y_max = self.y_max.saturating_sub(c.get_y());

		self.check().unwrap();
	}

	/// Subtracts the specified coordinates from the bounding box.
	///
	/// # Arguments
	///
	/// * `x` - The amount to subtract from the x-coordinates.
	/// * `y` - The amount to subtract from the y-coordinates.
	pub fn substract_u32(&mut self, x: u32, y: u32) {
		self.x_min = self.x_min.saturating_sub(x);
		self.y_min = self.y_min.saturating_sub(y);
		self.x_max = self.x_max.saturating_sub(x);
		self.y_max = self.y_max.saturating_sub(y);

		self.check().unwrap();
	}

	/// Scales down the bounding box by the specified factor.
	///
	/// # Arguments
	///
	/// * `scale` - The factor by which to scale down the bounding box.
	pub fn scale_down(&mut self, scale: u32) {
		self.x_min /= scale;
		self.y_min /= scale;
		self.x_max /= scale;
		self.y_max /= scale;
	}

	/// Checks if the bounding box contains the specified tile coordinate.
	///
	/// # Arguments
	///
	/// * `coord` - A reference to the tile coordinate.
	///
	/// # Returns
	///
	/// `true` if the bounding box contains the coordinate, `false` otherwise.
	pub fn contains(&self, coord: &TileCoord2) -> bool {
		(coord.get_x() >= self.x_min)
			&& (coord.get_x() <= self.x_max)
			&& (coord.get_y() >= self.y_min)
			&& (coord.get_y() <= self.y_max)
	}

	/// Checks if the bounding box contains the specified tile coordinate at the same zoom level.
	///
	/// # Arguments
	///
	/// * `coord` - A reference to the tile coordinate.
	///
	/// # Returns
	///
	/// `true` if the bounding box contains the coordinate at the same zoom level, `false` otherwise.
	pub fn contains3(&self, coord: &TileCoord3) -> bool {
		(coord.get_z() == self.level)
			&& (coord.get_x() >= self.x_min)
			&& (coord.get_x() <= self.x_max)
			&& (coord.get_y() >= self.y_min)
			&& (coord.get_y() <= self.y_max)
	}

	/// Returns the index of the specified tile coordinate within the bounding box.
	///
	/// # Arguments
	///
	/// * `coord` - A reference to the tile coordinate.
	///
	/// # Returns
	///
	/// The index of the tile coordinate within the bounding box.
	///
	/// # Panics
	///
	/// Panics if the coordinate is not within the bounding box.
	pub fn get_tile_index(&self, coord: &TileCoord2) -> usize {
		if !self.contains(coord) {
			panic!("coord '{coord:?}' is not in '{self:?}'")
		}

		let x = coord.get_x() - self.x_min;
		let y = coord.get_y() - self.y_min;
		let index = y * (self.x_max + 1 - self.x_min) + x;

		index as usize
	}

	/// Returns the tile coordinate at the specified index within the bounding box.
	///
	/// # Arguments
	///
	/// * `index` - The index of the tile coordinate.
	///
	/// # Returns
	///
	/// A `Result` containing the tile coordinate or an error if the index is out of bounds.
	pub fn get_coord2_by_index(&self, index: u32) -> Result<TileCoord2> {
		ensure!(index < self.count_tiles() as u32, "index out of bounds");

		let width = self.x_max + 1 - self.x_min;
		Ok(TileCoord2::new(
			index.rem(width) + self.x_min,
			index.div(width) + self.y_min,
		))
	}

	/// Returns the tile coordinate at the specified index within the bounding box and zoom level.
	///
	/// # Arguments
	///
	/// * `index` - The index of the tile coordinate.
	///
	/// # Returns
	///
	/// A `Result` containing the tile coordinate or an error if the index is out of bounds.
	pub fn get_coord3_by_index(&self, index: u32) -> Result<TileCoord3> {
		ensure!(index < self.count_tiles() as u32, "index out of bounds");

		let width = self.x_max + 1 - self.x_min;
		TileCoord3::new(index.rem(width) + self.x_min, index.div(width) + self.y_min, self.level)
	}

	/// Converts the bounding box to geographical coordinates.
	///
	/// # Arguments
	///
	/// * `z` - The zoom level of the bounding box.
	///
	/// # Returns
	///
	/// An array of four `f64` values representing the geographical bounding box.
	pub fn as_geo_bbox(&self, z: u8) -> [f64; 4] {
		let p_min = TileCoord3::new(self.x_min, self.y_max + 1, z).unwrap().as_geo();
		let p_max = TileCoord3::new(self.x_max + 1, self.y_min, z).unwrap().as_geo();

		[p_min[0], p_min[1], p_max[0], p_max[1]]
	}
}

impl fmt::Debug for TileBBox {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!(
			"{}: [{},{},{},{}] ({})",
			&self.level,
			&self.x_min,
			&self.y_min,
			&self.x_max,
			&self.y_max,
			&self.count_tiles()
		))
	}
}

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
		let bbox1 = TileBBox::from_geo(9, &[8.0653, 51.3563, 12.3528, 52.2564]).unwrap();
		let bbox2 = TileBBox::new(9, 267, 168, 273, 170).unwrap();
		assert_eq!(bbox1, bbox2);
	}

	#[test]
	fn from_geo_is_not_empty() {
		let bbox1 = TileBBox::from_geo(0, &[8.0, 51.0, 8.000001f64, 51.0]).unwrap();
		assert_eq!(bbox1.count_tiles(), 1);
		assert!(!bbox1.is_empty());

		let bbox2 = TileBBox::from_geo(14, &[-132.000001, -40.0, -132.0, -40.0]).unwrap();
		assert_eq!(bbox2.count_tiles(), 1);
		assert!(!bbox2.is_empty());
	}

	#[test]
	fn quarter_planet() {
		let geo_bbox2 = [0.0, -85.05112877980659f64, 180.0, 0.0];
		let mut geo_bbox0 = geo_bbox2;
		geo_bbox0[1] += 1e-10;
		geo_bbox0[2] -= 1e-10;
		for level in 1..32 {
			let level_bbox0 = TileBBox::from_geo(level, &geo_bbox0).unwrap();
			assert_eq!(level_bbox0.count_tiles(), 4u64.pow(level as u32 - 1));
			let geo_bbox1 = level_bbox0.as_geo_bbox(level);
			assert_eq!(geo_bbox1, geo_bbox2);
		}
	}

	#[test]
	fn sa_pacific() {
		let geo_bbox2 = [-180.0, -66.51326044311186f64, -90.0, 0.0];
		let mut geo_bbox0 = geo_bbox2;
		geo_bbox0[1] += 1e-10;
		geo_bbox0[2] -= 1e-10;

		for level in 2..32 {
			let level_bbox0 = TileBBox::from_geo(level, &geo_bbox0).unwrap();
			assert_eq!(level_bbox0.count_tiles(), 4u64.pow(level as u32 - 2));
			let geo_bbox1 = level_bbox0.as_geo_bbox(level);
			assert_eq!(geo_bbox1, geo_bbox2);
		}
	}

	#[test]
	fn get_tile_index() {
		let bbox = TileBBox::new(8, 100, 100, 199, 199).unwrap();
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(100, 100)), 0);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(101, 100)), 1);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(199, 100)), 99);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(100, 101)), 100);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(100, 199)), 9900);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(199, 199)), 9999);
	}

	#[test]
	fn boolean() {
		/*
			 #---#
		  #---# |
		  | | | |
		  | #-|-#
		  #---#
		*/
		let bbox1 = TileBBox::new(4, 0, 11, 2, 13).unwrap();
		let bbox2 = TileBBox::new(4, 1, 10, 3, 12).unwrap();

		let mut bbox1_intersect = bbox1.clone();
		bbox1_intersect.intersect_bbox(&bbox2);
		assert_eq!(bbox1_intersect, TileBBox::new(4, 1, 11, 2, 12).unwrap());

		let mut bbox1_union = bbox1;
		bbox1_union.union_bbox(&bbox2);
		assert_eq!(bbox1_union, TileBBox::new(4, 0, 10, 3, 13).unwrap());
	}

	#[test]
	fn include_tile() {
		let mut bbox = TileBBox::new(4, 0, 1, 2, 3).unwrap();
		bbox.include_tile(4, 5);
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
	fn iter_bbox_slices_99() {
		let bbox = TileBBox::new(10, 0, 1000, 99, 1003).unwrap();

		let vec: Vec<TileBBox> = bbox.iter_bbox_row_slices(99).collect();

		assert_eq!(vec.len(), 8);
		assert_eq!(vec[0], TileBBox::new(10, 0, 1000, 49, 1000).unwrap());
		assert_eq!(vec[1], TileBBox::new(10, 50, 1000, 99, 1000).unwrap());
		assert_eq!(vec[2], TileBBox::new(10, 0, 1001, 49, 1001).unwrap());
		assert_eq!(vec[3], TileBBox::new(10, 50, 1001, 99, 1001).unwrap());
		assert_eq!(vec[4], TileBBox::new(10, 0, 1002, 49, 1002).unwrap());
		assert_eq!(vec[5], TileBBox::new(10, 50, 1002, 99, 1002).unwrap());
		assert_eq!(vec[6], TileBBox::new(10, 0, 1003, 49, 1003).unwrap());
		assert_eq!(vec[7], TileBBox::new(10, 50, 1003, 99, 1003).unwrap());
	}

	#[test]
	fn iter_bbox_row_slices_100() {
		let bbox = TileBBox::new(10, 0, 1000, 99, 1003).unwrap();

		let vec: Vec<TileBBox> = bbox.iter_bbox_row_slices(100).collect();

		assert_eq!(vec.len(), 4);
		assert_eq!(vec[0], TileBBox::new(10, 0, 1000, 99, 1000).unwrap());
		assert_eq!(vec[1], TileBBox::new(10, 0, 1001, 99, 1001).unwrap());
		assert_eq!(vec[2], TileBBox::new(10, 0, 1002, 99, 1002).unwrap());
		assert_eq!(vec[3], TileBBox::new(10, 0, 1003, 99, 1003).unwrap());
	}

	#[test]
	fn iter_bbox_row_slices_199() {
		let bbox = TileBBox::new(10, 0, 1000, 99, 1003).unwrap();

		let vec: Vec<TileBBox> = bbox.iter_bbox_row_slices(199).collect();

		assert_eq!(vec.len(), 4);
		assert_eq!(vec[0], TileBBox::new(10, 0, 1000, 99, 1000).unwrap());
		assert_eq!(vec[1], TileBBox::new(10, 0, 1001, 99, 1001).unwrap());
		assert_eq!(vec[2], TileBBox::new(10, 0, 1002, 99, 1002).unwrap());
		assert_eq!(vec[3], TileBBox::new(10, 0, 1003, 99, 1003).unwrap());
	}

	#[test]
	fn iter_bbox_row_slices_200() {
		let bbox = TileBBox::new(10, 0, 1000, 99, 1003).unwrap();

		let vec: Vec<TileBBox> = bbox.iter_bbox_row_slices(200).collect();

		assert_eq!(vec.len(), 2);
		assert_eq!(vec[0], TileBBox::new(10, 0, 1000, 99, 1001).unwrap());
		assert_eq!(vec[1], TileBBox::new(10, 0, 1002, 99, 1003).unwrap());
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

		// border of (1, 1, 1, 1) should increase the size of the bbox by 1 in all directions
		bbox.add_border(1, 1, 1, 1);
		assert_eq!(bbox, TileBBox::new(8, 4, 9, 21, 31).unwrap());

		// border of (2, 3, 4, 5) should further increase the size of the bbox
		bbox.add_border(2, 3, 4, 5);
		assert_eq!(bbox, TileBBox::new(8, 2, 6, 25, 36).unwrap());

		// border of (0, 0, 0, 0) should not change the size of the bbox
		bbox.add_border(0, 0, 0, 0);
		assert_eq!(bbox, TileBBox::new(8, 2, 6, 25, 36).unwrap());

		// border of (0, 0, 0, 0) should not change the size of the bbox
		bbox.add_border(999, 999, 999, 999);
		assert_eq!(bbox, TileBBox::new(8, 0, 0, 255, 255).unwrap());

		// if bbox is empty, add_border should have no effect
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
	fn test_substract_coord2() {
		let mut bbox = TileBBox::new(4, 3, 3, 5, 5).unwrap();
		let coord = TileCoord2::new(1, 1);
		bbox.substract_coord2(&coord);
		assert_eq!(bbox, TileBBox::new(4, 2, 2, 4, 4).unwrap());
	}

	#[test]
	fn test_substract_u32() {
		let mut bbox = TileBBox::new(4, 3, 3, 5, 5).unwrap();
		bbox.substract_u32(1, 1);
		assert_eq!(bbox, TileBBox::new(4, 2, 2, 4, 4).unwrap());
	}

	#[test]
	fn test_get_max() {
		let bbox = TileBBox::new(4, 1, 1, 3, 3).unwrap();
		assert_eq!(bbox.max, 15);
	}
}
