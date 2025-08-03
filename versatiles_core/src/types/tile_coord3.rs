//! Utilities for three-dimensional tile coordinates (x, y, z) in a Web Mercator pyramid.
//!
//! Defines `TileCoord3` with methods for coordinate conversion, validation, and transformation.
//! This module defines the `TileCoord2` and `TileCoord3` structures, representing tile coordinates
//! in two and three dimensions, respectively. It includes methods for creating and manipulating
//! tile coordinates, converting them to geographic coordinates, and various utility functions.
//!
//! # Examples
//!
//! ```
//! use versatiles_core::{TileCoord2, TileCoord3};
//!
//! // Creating a new TileCoord2 instance
//! let coord2 = TileCoord2::new(3, 4);
//! assert_eq!(coord2.x, 3);
//! assert_eq!(coord2.y, 4);
//!
//! // Creating a new TileCoord3 instance
//! let coord3 = TileCoord3::new(5, 6, 7).unwrap();
//! assert_eq!(coord3.level, 5);
//! assert_eq!(coord3.x, 6);
//! assert_eq!(coord3.y, 7);
//!
//! // Converting TileCoord3 to geographic coordinates
//! let geo = coord3.as_geo();
//! ```

use crate::{GeoBBox, TileBBox, TileCoord2};
use anyhow::{Result, ensure};
use std::{
	f64::consts::PI as PI32,
	fmt::{self, Debug},
};

/// A 3D tile coordinate in a Web Mercator tile pyramid, with zoom level, x, and y indices.
///
/// Provides methods for geographic conversion, validation, indexing, and level transformations.
#[derive(Eq, PartialEq, Clone, Hash, Copy)]
pub struct TileCoord3 {
	pub x: u32,
	pub y: u32,
	pub level: u8,
}

#[allow(dead_code)]
impl TileCoord3 {
	/// Create a new `TileCoord3` at the given zoom `level` and tile indices `x`, `y`.
	///
	/// # Errors
	/// Returns an error if `level` > 31.
	pub fn new(level: u8, x: u32, y: u32) -> Result<TileCoord3> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		Ok(TileCoord3 { x, y, level })
	}

	/// Convert this tile coordinate to geographic longitude/latitude in degrees.
	pub fn as_geo(&self) -> [f64; 2] {
		let zoom: f64 = 2.0f64.powi(self.level as i32);

		[
			((self.x as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * (self.y as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		]
	}

	/// Return the geographic bounding box of this tile as `[west, south, east, north]`.
	pub fn as_geo_bbox(&self) -> GeoBBox {
		let zoom: f64 = 2.0f64.powi(self.level as i32);

		GeoBBox(
			((self.x as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * (self.y as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
			(((self.x + 1) as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * ((self.y + 1) as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		)
	}

	/// Discard the zoom level, returning the 2D tile coordinate (x, y).
	pub fn as_coord2(&self) -> TileCoord2 {
		TileCoord2 { x: self.x, y: self.y }
	}

	/// Serialize this coordinate to a compact JSON-like string `{x:…,y:…,z:…}`.
	pub fn as_json(&self) -> String {
		format!("{{x:{},y:{},z:{}}}", self.x, self.y, self.level)
	}

	/// Check whether `x` and `y` are within valid ranges for this zoom level.
	pub fn is_valid(&self) -> bool {
		if self.level > 30 {
			return false;
		};
		let max = 2u32.pow(self.level as u32);
		(self.x < max) && (self.y < max)
	}

	/// Compute a linear sort index combining zoom and x/y for total ordering.
	pub fn get_sort_index(&self) -> u64 {
		let size = 2u64.pow(self.level as u32);
		let offset = (size * size - 1) / 3;
		offset + size * self.y as u64 + self.x as u64
	}

	/// Scale down the x/y indices by integer `factor`, keeping the same zoom level.
	pub fn get_scaled_down(&self, factor: u32) -> TileCoord3 {
		TileCoord3 {
			level: self.level,
			x: self.x / factor,
			y: self.y / factor,
		}
	}

	/// Convert this tile coordinate and `tile_size` to a `TileBBox` covering the tile's grid.
	///
	/// # Errors
	/// Returns an error if bounding coordinates overflow.
	pub fn as_tile_bbox(&self, tile_size: u32) -> Result<TileBBox> {
		TileBBox::new(
			self.level,
			self.x,
			self.y,
			self.x + tile_size - 1,
			self.y + tile_size - 1,
		)
	}

	/// Change this coordinate to a new zoom `level`, scaling x/y accordingly.
	///
	/// If `level` > current, x/y are multiplied; if lower, x/y are divided.
	pub fn as_level(&self, level: u8) -> TileCoord3 {
		if level > self.level {
			let scale = 2u32.pow((level - self.level) as u32);
			TileCoord3 {
				x: self.x * scale,
				y: self.y * scale,
				level,
			}
		} else if level < self.level {
			let scale = 2u32.pow((self.level - level) as u32);
			TileCoord3 {
				x: self.x / scale,
				y: self.y / scale,
				level,
			}
		} else {
			*self // no change, same level
		}
	}
}

/// Custom `Debug` format as `TileCoord3(z, [x, y])` for readability.
impl Debug for TileCoord3 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("TileCoord3({}, [{}, {}])", &self.level, &self.x, &self.y))
	}
}

/// Lexicographic ordering: first by zoom `level`, then `y`, then `x`.
impl PartialOrd for TileCoord3 {
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
	use std::{
		collections::hash_map::DefaultHasher,
		hash::{Hash, Hasher},
	};

	#[test]
	fn partial_eq3() {
		let c = TileCoord3::new(2, 2, 2).unwrap();
		assert!(c.eq(&c));
		assert!(c.eq(&c.clone()));
		assert!(c.ne(&TileCoord3::new(1, 2, 2).unwrap()));
		assert!(c.ne(&TileCoord3::new(2, 1, 2).unwrap()));
		assert!(c.ne(&TileCoord3::new(2, 2, 1).unwrap()));
	}

	#[test]
	fn tilecoord3_new_and_getters() {
		let coord = TileCoord3::new(5, 3, 4).unwrap();
		assert_eq!(coord.x, 3);
		assert_eq!(coord.y, 4);
		assert_eq!(coord.level, 5);
	}

	#[test]
	fn tilecoord3_as_geo() {
		let coord = TileCoord3::new(5, 3, 4).unwrap();
		assert_eq!(coord.as_geo(), [-146.25, 79.17133464081945]);
		assert_eq!(
			coord.as_geo_bbox().as_array(),
			[-146.25, 79.17133464081945, -135.0, 76.84081641443098]
		);
	}

	#[test]
	fn tilecoord3_as_coord2() {
		let coord = TileCoord3::new(5, 3, 4).unwrap();
		let coord2 = coord.as_coord2();
		assert_eq!(coord2, TileCoord2::new(3, 4));
	}

	#[test]
	fn tilecoord3_is_valid() {
		let coord = TileCoord3::new(5, 3, 4).unwrap();
		assert!(coord.is_valid());
	}

	#[test]
	fn tilecoord3_get_sort_index() {
		let coord = TileCoord3::new(5, 3, 4).unwrap();
		assert_eq!(coord.get_sort_index(), 472);
	}

	#[test]
	fn hash() {
		let mut hasher = DefaultHasher::new();
		TileCoord3::new(2, 2, 2).unwrap().hash(&mut hasher);
		assert_eq!(hasher.finish(), 16217616760760983095);
	}

	#[test]
	fn partial_cmp3() {
		use std::cmp::Ordering;
		use std::cmp::Ordering::*;

		let check = |x: u32, y: u32, level: u8, order: Ordering| {
			let c1 = TileCoord3::new(2, 2, 2).unwrap();
			let c2 = TileCoord3::new(level, x, y).unwrap();
			assert_eq!(c2.partial_cmp(&c1), Some(order));
		};

		check(1, 1, 1, Less);
		check(2, 1, 1, Less);
		check(3, 1, 1, Less);
		check(1, 2, 1, Less);
		check(2, 2, 1, Less);
		check(3, 2, 1, Less);
		check(1, 3, 1, Less);
		check(2, 3, 1, Less);
		check(3, 3, 1, Less);

		check(1, 1, 2, Less);
		check(2, 1, 2, Less);
		check(3, 1, 2, Less);
		check(1, 2, 2, Less);
		check(2, 2, 2, Equal);
		check(3, 2, 2, Greater);
		check(1, 3, 2, Greater);
		check(2, 3, 2, Greater);
		check(3, 3, 2, Greater);

		check(1, 1, 3, Greater);
		check(2, 1, 3, Greater);
		check(3, 1, 3, Greater);
		check(1, 2, 3, Greater);
		check(2, 2, 3, Greater);
		check(3, 2, 3, Greater);
		check(1, 3, 3, Greater);
		check(2, 3, 3, Greater);
		check(3, 3, 3, Greater);
	}

	#[test]
	fn tilecoord3_new_level_error() {
		// Level > 31 should error
		assert!(TileCoord3::new(32, 0, 0).is_err());
	}

	#[test]
	fn tilecoord3_is_valid_false_cases() {
		// Level 31 is considered invalid by is_valid()
		let coord = TileCoord3::new(31, 0, 0).unwrap();
		assert!(!coord.is_valid());
		// x out of bounds for level 1 (max index = 1)
		let coord2 = TileCoord3::new(1, 2, 0).unwrap();
		assert!(!coord2.is_valid());
		// y out of bounds for level 1
		let coord3 = TileCoord3::new(1, 0, 2).unwrap();
		assert!(!coord3.is_valid());
	}

	#[test]
	fn tilecoord3_as_json_and_scaled_down() {
		let coord = TileCoord3::new(2, 5, 6).unwrap();
		// Test JSON serialization
		assert_eq!(coord.as_json(), "{x:5,y:6,z:2}");
		// Test scaling down by a factor
		let scaled = coord.get_scaled_down(5);
		assert_eq!(scaled, TileCoord3::new(2, 1, 1).unwrap());
		// Scaling by 1 returns same
		assert_eq!(coord.get_scaled_down(1), coord);
	}

	#[test]
	fn tilecoord3_as_tile_bbox_and_as_level() {
		let coord = TileCoord3::new(3, 1, 2).unwrap();
		// as_tile_bbox with tile_size=4: x..x+3, y..y+3
		let bbox = coord.as_tile_bbox(4).unwrap();
		assert_eq!(bbox, TileBBox::new(3, 1, 2, 4, 5).unwrap());
		// as_level upscales and downscales correctly
		let up = coord.as_level(5);
		assert_eq!(up, TileCoord3::new(5, 4, 8).unwrap());
		let down = coord.as_level(2);
		assert_eq!(down, TileCoord3::new(2, 0, 1).unwrap());
		// same level returns identical
		assert_eq!(coord.as_level(3), coord);
	}

	#[test]
	fn tilecoord3_debug_format() {
		let coord = TileCoord3::new(4, 7, 8).unwrap();
		// Expect format: TileCoord3(level, [x, y])
		assert_eq!(format!("{:?}", coord), "TileCoord3(4, [7, 8])");
	}
}
