//! Three-dimensional tile coordinates in a Web Mercator pyramid
//!
//! This module provides the [`TileCoord`] type for representing tile coordinates in a
//! Web Mercator tile pyramid. It includes methods for:
//! - Creating and validating tile coordinates
//! - Converting between tile and geographic coordinates
//! - Transforming coordinates across zoom levels
//! - Computing spatial indices and relationships
//!
//! # Examples
//!
//! ```
//! use versatiles_core::TileCoord;
//!
//! // Create a new tile coordinate
//! let coord = TileCoord::new(5, 6, 7).unwrap();
//! assert_eq!(coord.level, 5);
//! assert_eq!(coord.x, 6);
//! assert_eq!(coord.y, 7);
//!
//! // Convert to geographic coordinates
//! let [lon, lat] = coord.as_geo();
//! println!("Tile is at {}, {}", lon, lat);
//!
//! // Scale to a different zoom level
//! let zoomed = coord.at_level(7);
//! assert_eq!(zoomed.level, 7);
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
	/// The zoom level of the tile.
	pub level: u8,
	/// The x index of the tile.
	pub x: u32,
	/// The y index of the tile.
	pub y: u32,
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
		Ok(TileCoord { level, x, y })
	}

	/// Create a new `TileCoord`, clamping the x and y indices to valid bounds for the given level.
	///
	/// Unlike [`new`](Self::new), this method clamps out-of-bounds coordinates instead of returning an error.
	///
	/// # Arguments
	///
	/// * `level` - The zoom level
	/// * `x` - The x coordinate (will be clamped to `[0, 2^level - 1]`)
	/// * `y` - The y coordinate (will be clamped to `[0, 2^level - 1]`)
	///
	/// # Errors
	///
	/// Returns an error if `level` > 31.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// // x=1000 is out of bounds for level 5 (max is 31), so it gets clamped
	/// let coord = TileCoord::new_clamped(5, 1000, 10).unwrap();
	/// assert_eq!(coord.x, 31);
	/// assert_eq!(coord.y, 10);
	/// ```
	pub fn new_clamped(level: u8, x: u32, y: u32) -> Result<TileCoord> {
		let max = 2u32.pow(u32::from(level)) - 1;
		TileCoord::new(level, x.min(max), y.min(max))
	}

	/// Create a `TileCoord` from geographic coordinates (longitude, latitude) at a given zoom level.
	///
	/// Uses Web Mercator projection to convert from WGS84 coordinates to tile indices.
	///
	/// # Arguments
	///
	/// * `x` - Longitude in degrees, range `[-180, 180]`
	/// * `y` - Latitude in degrees, range `[-90, 90]`
	/// * `z` - Zoom level, range `[0, 31]`
	///
	/// # Errors
	///
	/// Returns an error if coordinates are out of valid ranges.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// // Berlin coordinates at zoom 10
	/// let coord = TileCoord::from_geo(13.404954, 52.520008, 10).unwrap();
	/// assert_eq!(coord.level, 10);
	/// ```
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
			#[allow(clippy::cast_possible_truncation)] // Safe: clamped to valid tile range
			u32::try_from(x.min(zoom - 1.0).max(0.0).floor() as i64)?,
			#[allow(clippy::cast_possible_truncation)] // Safe: clamped to valid tile range
			u32::try_from(y.min(zoom - 1.0).max(0.0).floor() as i64)?,
		)
	}

	/// Convert tile coordinates to geographic coordinates (longitude, latitude) in degrees.
	///
	/// Returns the northwest corner of the tile in WGS84 coordinates.
	///
	/// # Arguments
	///
	/// * `level` - The zoom level
	/// * `x` - The x tile index
	/// * `y` - The y tile index
	///
	/// # Returns
	///
	/// `[longitude, latitude]` in degrees
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// let [lon, lat] = TileCoord::coord_to_geo(10, 1, 1020);
	/// // Returns coordinates for the northwest corner of the tile
	/// assert_eq!(format!("{lon:.5}"), "-179.64844");
	/// assert_eq!(format!("{lat:.5}"), "-84.92832");
	/// ```
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
		self.to_tile_bbox().to_geo_bbox().unwrap()
	}

	/// Serialize this coordinate to a compact JSON-like string `{x:…,y:…,z:…}`.
	#[must_use]
	pub fn as_json(&self) -> String {
		format!("{{\"z\":{},\"x\":{},\"y\":{}}}", self.level, self.x, self.y)
	}

	/// Compute a linear sort index combining zoom and x/y for total ordering.
	#[must_use]
	pub fn sort_index(&self) -> u64 {
		let size = 2u64.pow(u32::from(self.level));
		let offset = (size * size - 1) / 3;
		offset + size * u64::from(self.y) + u64::from(self.x)
	}

	/// Scale down the x/y indices by integer `factor`, keeping the same zoom level.
	#[must_use]
	pub fn scaled_down(&self, factor: u32) -> TileCoord {
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
	pub fn to_tile_bbox(&self) -> TileBBox {
		TileBBox::from_min_and_size(self.level, self.x, self.y, 1, 1).unwrap()
	}

	/// Change this coordinate to a new zoom `level`, scaling x/y accordingly.
	///
	/// If `level` > current, x/y are multiplied; if lower, x/y are divided.
	#[must_use]
	pub fn at_level(&self, level: u8) -> TileCoord {
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

	/// Round down the x and y coordinates to the nearest multiple of `size`.
	///
	/// This is useful for aligning tiles to a grid of a given size.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// let mut coord = TileCoord::new(5, 17, 23).unwrap();
	/// coord.floor(8);
	/// assert_eq!(coord.x, 16); // 17 floored to nearest multiple of 8
	/// assert_eq!(coord.y, 16); // 23 floored to nearest multiple of 8
	/// ```
	pub fn floor(&mut self, size: u32) {
		self.x = (self.x / size) * size;
		self.y = (self.y / size) * size;
	}

	/// Round up the x and y coordinates to the nearest multiple of `size`, minus one.
	///
	/// This aligns tiles to the upper boundary of a grid cell.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// let mut coord = TileCoord::new(5, 17, 23).unwrap();
	/// coord.ceil(8);
	/// assert_eq!(coord.x, 23); // (17/8 + 1) * 8 - 1
	/// assert_eq!(coord.y, 23); // (23/8 + 1) * 8 - 1
	/// ```
	pub fn ceil(&mut self, size: u32) {
		self.x = (self.x / size + 1) * size - 1;
		self.y = (self.y / size + 1) * size - 1;
	}

	/// Shift the tile coordinate by the given deltas, clamping to valid bounds.
	///
	/// # Arguments
	///
	/// * `dx` - The number of tiles to shift in the x direction (can be negative)
	/// * `dy` - The number of tiles to shift in the y direction (can be negative)
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// let mut coord = TileCoord::new(5, 16, 16).unwrap();
	/// coord.shift_by(5, -3);
	/// assert_eq!(coord.x, 21);
	/// assert_eq!(coord.y, 13);
	/// ```
	pub fn shift_by(&mut self, dx: i64, dy: i64) {
		let max_value = 2i64.pow(u32::from(self.level)) - 1;
		self.x = u32::try_from((i64::from(self.x) + dx).max(0).min(max_value)).expect("clamped value must fit in u32");
		self.y = u32::try_from((i64::from(self.y) + dy).max(0).min(max_value)).expect("clamped value must fit in u32");
	}

	/// Get the maximum valid x or y coordinate for this tile's zoom level.
	///
	/// Returns `2^level - 1`.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// let coord = TileCoord::new(5, 10, 15).unwrap();
	/// assert_eq!(coord.max_value(), 31); // 2^5 - 1
	/// ```
	#[must_use]
	pub fn max_value(&self) -> u32 {
		(1u32 << self.level) - 1
	}

	/// Flip the y coordinate vertically within the tile grid.
	///
	/// This is useful for converting between TMS (y increasing upward) and
	/// XYZ (y increasing downward) tile schemes.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// let mut coord = TileCoord::new(3, 1, 2).unwrap();
	/// coord.flip_y();
	/// assert_eq!(coord.y, 5); // 7 (max) - 2 = 5
	/// ```
	pub fn flip_y(&mut self) {
		self.y = self.max_value() - self.y;
	}

	/// Swap the x and y coordinates.
	///
	/// This can be useful for transposing tile grids.
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// let mut coord = TileCoord::new(5, 10, 20).unwrap();
	/// coord.swap_xy();
	/// assert_eq!(coord.x, 20);
	/// assert_eq!(coord.y, 10);
	/// ```
	pub fn swap_xy(&mut self) {
		std::mem::swap(&mut self.x, &mut self.y);
	}

	/// Return a new coordinate at the parent zoom level (level - 1).
	///
	/// The x and y coordinates are divided by 2 to move up one level in the tile pyramid.
	///
	/// # Errors
	///
	/// Returns an error if the current level is 0 (no parent level exists).
	///
	/// # Examples
	///
	/// ```
	/// use versatiles_core::TileCoord;
	///
	/// let coord = TileCoord::new(5, 16, 20).unwrap();
	/// let parent = coord.to_level_decreased().unwrap();
	/// assert_eq!(parent.level, 4);
	/// assert_eq!(parent.x, 8);
	/// assert_eq!(parent.y, 10);
	/// ```
	pub fn to_level_decreased(&self) -> Result<TileCoord> {
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
	fn tilecoord_sort_index() {
		let coord = TileCoord::new(5, 3, 4).unwrap();
		assert_eq!(coord.sort_index(), 472);
	}

	#[test]
	fn hash() {
		let mut hasher = DefaultHasher::new();
		TileCoord::new(2, 2, 2).unwrap().hash(&mut hasher);
		assert_eq!(hasher.finish(), 13950038470645857615);
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
		assert_eq!(coord.as_json(), "{\"z\":4,\"x\":5,\"y\":6}");
		// Test scaling down by a factor
		let scaled = coord.scaled_down(5);
		assert_eq!(scaled, TileCoord::new(4, 1, 1).unwrap());
		// Scaling by 1 returns same
		assert_eq!(coord.scaled_down(1), coord);
	}

	#[test]
	fn tilecoord_as_tile_bbox_and_as_level() {
		let coord = TileCoord::new(3, 1, 2).unwrap();
		// as_tile_bbox with tile_size=4: x..x+3, y..y+3
		let bbox = coord.to_tile_bbox();
		assert_eq!(bbox, TileBBox::from_min_and_max(3, 1, 2, 1, 2).unwrap());
		// as_level upscales and downscales correctly
		let up = coord.at_level(5);
		assert_eq!(up, TileCoord::new(5, 4, 8).unwrap());
		let down = coord.at_level(2);
		assert_eq!(down, TileCoord::new(2, 0, 1).unwrap());
		// same level returns identical
		assert_eq!(coord.at_level(3), coord);
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
