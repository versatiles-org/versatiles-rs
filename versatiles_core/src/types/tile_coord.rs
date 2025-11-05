//! Utilities for three-dimensional tile coordinates (x, y, z) in a Web Mercator pyramid.
//!
//! Defines `TileCoord` with methods for coordinate conversion, validation, and transformation.
//! This module defines the `TileCoord` structures, representing tile coordinates
//! in two dimensions, respectively. It includes methods for creating and manipulating
//! tile coordinates, converting them to geographic coordinates, and various utility functions.
//!
//! # Examples
//!
//! ```
//! use versatiles_core::TileCoord;
//!
//! // Creating a new TileCoord instance
//! let coord = TileCoord::new(5, 6, 7).unwrap();
//! assert_eq!(coord.level, 5);
//! assert_eq!(coord.x, 6);
//! assert_eq!(coord.y, 7);
//!
//! // Converting TileCoord to geographic coordinates
//! let geo = coord.as_geo();
//! ```

use crate::{GeoBBox, TileBBox};
use anyhow::{Result, ensure};
use std::{
	f64::consts::PI as PI32,
	fmt::{self, Debug},
};
use versatiles_derive::context;

/// A 3D tile coordinate in a Web Mercator tile pyramid, with zoom level, x, and y indices.
///
/// Provides methods for geographic conversion, validation, indexing, and level transformations.
#[derive(Eq, PartialEq, Clone, Hash, Copy)]
pub struct TileCoord {
	pub x: u32,
	pub y: u32,
	pub level: u8,
}

#[allow(dead_code)]
impl TileCoord {
	/// Create a new `TileCoord` at the given zoom `level` and tile indices `x`, `y`.
	///
	/// # Errors
	/// Returns an error if `level` > 31.
	pub fn new(level: u8, x: u32, y: u32) -> Result<TileCoord> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		let max = 2u32.pow(u32::from(level));
		ensure!(x < max, "x ({x}) out of bounds for level {level}");
		ensure!(y < max, "y ({y}) out of bounds for level {level}");
		Ok(TileCoord { x, y, level })
	}

	pub fn new_clamped(level: u8, x: u32, y: u32) -> Result<TileCoord> {
		let max = 2u32.pow(u32::from(level)) - 1;
		TileCoord::new(level, x.min(max), y.min(max))
	}

	#[context("Failed to convert geo coordinates ({x}, {y}, {z}) to TileCoord")]
	pub fn from_geo(x: f64, y: f64, z: u8) -> Result<TileCoord> {
		ensure!(z <= 31, "z ({z}) must be <= 31");
		ensure!(x >= -180., "x ({x}) must be >= -180");
		ensure!(x <= 180., "x ({x}) must be <= 180");
		ensure!(y >= -90., "y ({y}) must be >= -90");
		ensure!(y <= 90., "y ({y}) must be <= 90");

		let zoom: f64 = 2.0f64.powi(i32::from(z));
		let x = zoom * (x / 360.0 + 0.5);
		let y = zoom * (0.5 - 0.5 * (y * PI32 / 360.0 + PI32 / 4.0).tan().ln() / PI32);

		TileCoord::new(
			z,
			x.min(zoom - 1.0).max(0.0).floor() as u32,
			y.min(zoom - 1.0).max(0.0).floor() as u32,
		)
	}

	pub fn coord_to_geo(level: u8, x: u32, y: u32) -> [f64; 2] {
		let zoom: f64 = 2.0f64.powi(i32::from(level));
		[
			(f64::from(x) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * f64::from(y) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		]
	}

	/// Convert this tile coordinate to geographic longitude/latitude in degrees.
	#[must_use]
	pub fn as_geo(&self) -> [f64; 2] {
		TileCoord::coord_to_geo(self.level, self.x, self.y)
	}

	/// Return the geographic bounding box of this tile as `[west, south, east, north]`.
	#[must_use]
	pub fn to_geo_bbox(&self) -> GeoBBox {
		self.as_tile_bbox().to_geo_bbox().unwrap()
	}

	/// Serialize this coordinate to a compact JSON-like string `{x:…,y:…,z:…}`.
	#[must_use]
	pub fn as_json(&self) -> String {
		format!("{{x:{},y:{},z:{}}}", self.x, self.y, self.level)
	}

	/// Compute a linear sort index combining zoom and x/y for total ordering.
	#[must_use]
	pub fn get_sort_index(&self) -> u64 {
		let size = 2u64.pow(u32::from(self.level));
		let offset = (size * size - 1) / 3;
		offset + size * u64::from(self.y) + u64::from(self.x)
	}

	/// Scale down the x/y indices by integer `factor`, keeping the same zoom level.
	#[must_use]
	pub fn get_scaled_down(&self, factor: u32) -> TileCoord {
		TileCoord {
			level: self.level,
			x: self.x / factor,
			y: self.y / factor,
		}
	}

	/// Convert this tile coordinate and `tile_size` to a `TileBBox` covering the tile's grid.
	///
	/// # Errors
	/// Returns an error if bounding coordinates overflow.
	pub fn as_tile_bbox(&self) -> TileBBox {
		TileBBox::from_min_and_size(self.level, self.x, self.y, 1, 1).unwrap()
	}

	/// Change this coordinate to a new zoom `level`, scaling x/y accordingly.
	///
	/// If `level` > current, x/y are multiplied; if lower, x/y are divided.
	#[must_use]
	pub fn as_level(&self, level: u8) -> TileCoord {
		assert!(level <= 31, "level ({level}) must be <= 31");
		if level > self.level {
			let scale = 2u32.pow(u32::from(level - self.level));
			TileCoord {
				x: self.x * scale,
				y: self.y * scale,
				level,
			}
		} else if level < self.level {
			let scale = 2u32.pow(u32::from(self.level - level));
			TileCoord {
				x: self.x / scale,
				y: self.y / scale,
				level,
			}
		} else {
			*self // no change, same level
		}
	}

	pub fn floor(&mut self, size: u32) {
		self.x = (self.x / size) * size;
		self.y = (self.y / size) * size;
	}

	pub fn ceil(&mut self, size: u32) {
		self.x = (self.x / size + 1) * size - 1;
		self.y = (self.y / size + 1) * size - 1;
	}

	pub fn shift_by(&mut self, dx: i64, dy: i64) {
		let max_value = 2i64.pow(u32::from(self.level)) - 1;
		self.x = (i64::from(self.x) + dx).max(0).min(max_value) as u32;
		self.y = (i64::from(self.y) + dy).max(0).min(max_value) as u32;
	}

	#[must_use]
	pub fn max_value(&self) -> u32 {
		(1u32 << self.level) - 1
	}
	pub fn flip_y(&mut self) {
		self.y = self.max_value() - self.y;
	}
	pub fn swap_xy(&mut self) {
		std::mem::swap(&mut self.x, &mut self.y);
	}
	pub fn as_level_decreased(&self) -> Result<TileCoord> {
		ensure!(self.level > 0, "cannot decrease level below 0");
		TileCoord::new(self.level - 1, self.x / 2, self.y / 2)
	}
}

/// Custom `Debug` format as `TileCoord(z, [x, y])` for readability.
impl Debug for TileCoord {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("TileCoord({}, [{}, {}])", &self.level, &self.x, &self.y))
	}
}

/// Lexicographic ordering: first by zoom `level`, then `y`, then `x`.
impl PartialOrd for TileCoord {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		match self.level.partial_cmp(&other.level) {
			Some(core::cmp::Ordering::Equal) => {}
			ord => return ord,
		}
		match self.y.partial_cmp(&other.y) {
			Some(core::cmp::Ordering::Equal) => {}
			ord => return ord,
		}
		self.x.partial_cmp(&other.x)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;
	use std::{
		cmp::Ordering::{self, *},
		collections::hash_map::DefaultHasher,
		hash::{Hash, Hasher},
	};

	#[test]
	fn partial_eq() {
		let c = TileCoord::new(2, 2, 2).unwrap();
		assert!(c.eq(&c));
		assert!(c.eq(&c.clone()));
		assert!(c.ne(&TileCoord::new(3, 2, 2).unwrap()));
		assert!(c.ne(&TileCoord::new(2, 3, 2).unwrap()));
		assert!(c.ne(&TileCoord::new(2, 2, 3).unwrap()));
	}

	#[test]
	fn tilecoord_new_and_getters() {
		let coord = TileCoord::new(5, 3, 4).unwrap();
		assert_eq!(coord.x, 3);
		assert_eq!(coord.y, 4);
		assert_eq!(coord.level, 5);
	}

	#[test]
	fn tilecoord_as_geo() {
		let coord = TileCoord::new(5, 3, 4).unwrap();
		assert_eq!(coord.as_geo(), [-146.25, 79.17133464081945]);
		assert_eq!(
			coord.to_geo_bbox().as_array(),
			[-146.25, 76.84081641443098, -135.0, 79.17133464081945]
		);
	}

	#[test]
	fn tilecoord_get_sort_index() {
		let coord = TileCoord::new(5, 3, 4).unwrap();
		assert_eq!(coord.get_sort_index(), 472);
	}

	#[test]
	fn hash() {
		let mut hasher = DefaultHasher::new();
		TileCoord::new(2, 2, 2).unwrap().hash(&mut hasher);
		assert_eq!(hasher.finish(), 16217616760760983095);
	}

	#[rstest]
	#[case(1, 0, 0, Less)]
	#[case(1, 0, 1, Less)]
	#[case(1, 1, 0, Less)]
	#[case(1, 1, 1, Less)]
	#[case(2, 1, 1, Less)]
	#[case(2, 1, 2, Less)]
	#[case(2, 1, 3, Greater)]
	#[case(2, 2, 1, Less)]
	#[case(2, 2, 2, Equal)]
	#[case(2, 2, 3, Greater)]
	#[case(2, 3, 1, Less)]
	#[case(2, 3, 2, Greater)]
	#[case(2, 3, 3, Greater)]
	#[case(3, 1, 1, Greater)]
	#[case(3, 1, 2, Greater)]
	#[case(3, 1, 3, Greater)]
	#[case(3, 2, 1, Greater)]
	#[case(3, 2, 2, Greater)]
	#[case(3, 2, 3, Greater)]
	#[case(3, 3, 1, Greater)]
	#[case(3, 3, 2, Greater)]
	#[case(3, 3, 3, Greater)]
	fn partial_cmp_cases(#[case] level: u8, #[case] x: u32, #[case] y: u32, #[case] expected: Ordering) {
		let c1 = TileCoord::new(2, 2, 2).unwrap();
		let c2 = TileCoord::new(level, x, y).unwrap();
		assert_eq!(c2.partial_cmp(&c1), Some(expected));
	}

	#[test]
	fn tilecoord_new_level_error() {
		// Level > 31 should error
		assert!(TileCoord::new(32, 0, 0).is_err());
	}

	#[test]
	fn tilecoord_as_json_and_scaled_down() {
		let coord = TileCoord::new(4, 5, 6).unwrap();
		// Test JSON serialization
		assert_eq!(coord.as_json(), "{x:5,y:6,z:4}");
		// Test scaling down by a factor
		let scaled = coord.get_scaled_down(5);
		assert_eq!(scaled, TileCoord::new(4, 1, 1).unwrap());
		// Scaling by 1 returns same
		assert_eq!(coord.get_scaled_down(1), coord);
	}

	#[test]
	fn tilecoord_as_tile_bbox_and_as_level() {
		let coord = TileCoord::new(3, 1, 2).unwrap();
		// as_tile_bbox with tile_size=4: x..x+3, y..y+3
		let bbox = coord.as_tile_bbox();
		assert_eq!(bbox, TileBBox::from_min_and_max(3, 1, 2, 1, 2).unwrap());
		// as_level upscales and downscales correctly
		let up = coord.as_level(5);
		assert_eq!(up, TileCoord::new(5, 4, 8).unwrap());
		let down = coord.as_level(2);
		assert_eq!(down, TileCoord::new(2, 0, 1).unwrap());
		// same level returns identical
		assert_eq!(coord.as_level(3), coord);
	}

	#[test]
	fn tilecoord_debug_format() {
		let coord = TileCoord::new(4, 7, 8).unwrap();
		// Expect format: TileCoord(level, [x, y])
		assert_eq!(format!("{coord:?}"), "TileCoord(4, [7, 8])");
	}
	#[test]
	fn tilecoord_flip_y() {
		let mut c = TileCoord::new(3, 1, 2).unwrap();
		c.flip_y();
		assert_eq!(c, TileCoord::new(3, 1, 5).unwrap());
	}

	#[test]
	fn tilecoord_swap_xy() {
		let mut coord = TileCoord::new(5, 3, 4).unwrap();
		coord.swap_xy();
		assert_eq!(coord, TileCoord::new(5, 4, 3).unwrap());
	}
}
