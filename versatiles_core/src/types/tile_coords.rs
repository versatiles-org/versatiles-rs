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

use anyhow::{Result, ensure};
use std::{
	f64::consts::PI as PI32,
	fmt::{self, Debug},
	ops::{Add, Sub},
};

use crate::TileBBox;

use super::GeoBBox;

#[derive(Eq, PartialEq, Clone, Hash)]
pub struct TileCoord2 {
	pub x: u32,
	pub y: u32,
}

#[allow(dead_code)]
impl TileCoord2 {
	pub fn new(x: u32, y: u32) -> TileCoord2 {
		TileCoord2 { x, y }
	}

	pub fn from_geo(x: f64, y: f64, z: u8, round_up: bool) -> Result<TileCoord2> {
		ensure!(z <= 31, "z {z} must be <= 31");
		ensure!(x >= -180., "x must be >= -180");
		ensure!(x <= 180., "x must be <= 180");
		ensure!(y >= -90., "y must be >= -90");
		ensure!(y <= 90., "y must be <= 90");

		let zoom: f64 = 2.0f64.powi(z as i32);
		let mut x = zoom * (x / 360.0 + 0.5);
		let mut y = zoom * (0.5 - 0.5 * (y * PI32 / 360.0 + PI32 / 4.0).tan().ln() / PI32);

		// add/subtract a little offset to compensate for floating point rounding issues
		if round_up {
			x = x.sub(1e-6).floor();
			y = y.sub(1e-6).floor();
		} else {
			x = x.add(1e-6).floor();
			y = y.add(1e-6).floor();
		}

		Ok(TileCoord2 {
			x: x.min(zoom - 1.0).max(0.0) as u32,
			y: y.min(zoom - 1.0).max(0.0) as u32,
		})
	}

	pub fn subtract(&mut self, c: &TileCoord2) {
		self.x -= c.x;
		self.y -= c.y;
	}

	pub fn scale_by(&mut self, s: u32) {
		self.x *= s;
		self.y *= s;
	}
}

impl fmt::Debug for TileCoord2 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("TileCoord2({}, {})", &self.x, &self.y))
	}
}

impl PartialOrd for TileCoord2 {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		match self.y.partial_cmp(&other.y) {
			Some(core::cmp::Ordering::Equal) => {}
			ord => return ord,
		}
		self.x.partial_cmp(&other.x)
	}
}

#[derive(Eq, PartialEq, Clone, Hash, Copy)]
pub struct TileCoord3 {
	pub x: u32,
	pub y: u32,
	pub level: u8,
}

#[allow(dead_code)]
impl TileCoord3 {
	pub fn new(level: u8, x: u32, y: u32) -> Result<TileCoord3> {
		ensure!(level <= 31, "level ({level}) must be <= 31");
		Ok(TileCoord3 { x, y, level })
	}

	pub fn as_geo(&self) -> [f64; 2] {
		let zoom: f64 = 2.0f64.powi(self.level as i32);

		[
			((self.x as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * (self.y as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		]
	}

	pub fn as_geo_bbox(&self) -> GeoBBox {
		let zoom: f64 = 2.0f64.powi(self.level as i32);

		GeoBBox(
			((self.x as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * (self.y as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
			(((self.x + 1) as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * ((self.y + 1) as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		)
	}

	pub fn as_coord2(&self) -> TileCoord2 {
		TileCoord2 { x: self.x, y: self.y }
	}

	pub fn as_json(&self) -> String {
		format!("{{x:{},y:{},z:{}}}", self.x, self.y, self.level)
	}

	pub fn is_valid(&self) -> bool {
		if self.level > 30 {
			return false;
		};
		let max = 2u32.pow(self.level as u32);
		(self.x < max) && (self.y < max)
	}

	pub fn get_sort_index(&self) -> u64 {
		let size = 2u64.pow(self.level as u32);
		let offset = (size * size - 1) / 3;
		offset + size * self.y as u64 + self.x as u64
	}

	pub fn get_scaled_down(&self, factor: u32) -> TileCoord3 {
		TileCoord3 {
			level: self.level,
			x: self.x / factor,
			y: self.y / factor,
		}
	}

	pub fn as_tile_bbox(&self, tile_size: u32) -> Result<TileBBox> {
		TileBBox::new(
			self.level,
			self.x,
			self.y,
			self.x + tile_size - 1,
			self.y + tile_size - 1,
		)
	}

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

impl Debug for TileCoord3 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("TileCoord3({}, [{}, {}])", &self.level, &self.x, &self.y))
	}
}

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
	fn from_geo() {
		let test = |z: u8, x: u32, y: u32, xf: f64, yf: f64| {
			assert_eq!(TileCoord2::from_geo(xf, yf, z, false).unwrap(), TileCoord2::new(x, y));
			assert_eq!(TileCoord2::from_geo(xf, yf, z, true).unwrap(), TileCoord2::new(x, y));
		};

		test(9, 267, 168, 8.0653, 52.2564);
		test(9, 273, 170, 12.3528, 51.3563);

		test(12, 1997, 1233, -4.43515, 58.0042);
		test(12, 2280, 1476, 20.4395, 44.8029);
	}

	#[test]
	fn debug() {
		assert_eq!(format!("{:?}", TileCoord2::new(1, 2)), "TileCoord2(1, 2)");
		assert_eq!(
			format!("{:?}", TileCoord3::new(3, 1, 2).unwrap()),
			"TileCoord3(3, [1, 2])"
		);
	}

	#[test]
	fn partial_eq2() {
		let c = TileCoord2::new(2, 2);
		assert!(c.eq(&c));
		assert!(c.eq(&c.clone()));
		assert!(c.ne(&TileCoord2::new(1, 2)));
		assert!(c.ne(&TileCoord2::new(2, 1)));
	}

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
	fn tilecoord2_new_and_getters() {
		let coord = TileCoord2::new(3, 4);
		assert_eq!(coord.x, 3);
		assert_eq!(coord.y, 4);
	}

	#[test]
	fn tilecoord2_subtract() {
		let mut coord1 = TileCoord2::new(5, 7);
		let coord2 = TileCoord2::new(2, 3);
		coord1.subtract(&coord2);
		assert_eq!(coord1, TileCoord2::new(3, 4));
	}

	#[test]
	fn tilecoord2_scale_by() {
		let mut coord = TileCoord2::new(3, 4);
		coord.scale_by(2);
		assert_eq!(coord, TileCoord2::new(6, 8));
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
		TileCoord2::new(2, 2).hash(&mut hasher);
		TileCoord3::new(2, 2, 2).unwrap().hash(&mut hasher);
		assert_eq!(hasher.finish(), 8202047236025635059);
	}

	#[test]
	fn partial_cmp2() {
		use std::cmp::Ordering;
		use std::cmp::Ordering::*;

		let check = |x: u32, y: u32, order: Ordering| {
			let c1 = TileCoord2::new(2, 2);
			let c2 = TileCoord2::new(x, y);
			assert_eq!(c2.partial_cmp(&c1), Some(order));
		};

		check(1, 1, Less);
		check(2, 1, Less);
		check(3, 1, Less);
		check(1, 2, Less);
		check(2, 2, Equal);
		check(3, 2, Greater);
		check(1, 3, Greater);
		check(2, 3, Greater);
		check(3, 3, Greater);
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
}
