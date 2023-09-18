use std::{
	f64::consts::PI as PI32,
	fmt::{self, Debug},
};

#[derive(Eq, PartialEq, Clone, Hash)]
pub struct TileCoord2 {
	x: u32,
	y: u32,
}
impl TileCoord2 {
	pub fn new(x: u32, y: u32) -> TileCoord2 {
		TileCoord2 { x, y }
	}
	pub fn from_geo(x: f64, y: f64, z: u8) -> TileCoord2 {
		assert!(z <= 31, "z {z} must be <= 31");

		let zoom: f64 = 2.0f64.powi(z as i32);
		let x = zoom * (x / 360.0 + 0.5);
		let y = zoom * (0.5 - 0.5 * (y * PI32 / 360.0 + PI32 / 4.0).tan().ln() / PI32);

		TileCoord2 {
			x: x.floor().min(zoom - 1.0).max(0.0) as u32,
			y: y.floor().min(zoom - 1.0).max(0.0) as u32,
		}
	}
	pub fn get_x(&self) -> u32 {
		self.x
	}
	pub fn get_y(&self) -> u32 {
		self.y
	}
	#[allow(dead_code)]
	pub fn substract(&mut self, c: &TileCoord2) {
		self.x -= c.x;
		self.y -= c.y;
	}
	#[allow(dead_code)]
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
	pub z: u8,
}
impl TileCoord3 {
	pub fn new(x: u32, y: u32, z: u8) -> TileCoord3 {
		assert!(z <= 31, "z ({z}) must be <= 31");
		TileCoord3 { x, y, z }
	}
	pub fn get_x(&self) -> u32 {
		self.x
	}
	pub fn get_y(&self) -> u32 {
		self.y
	}
	pub fn get_z(&self) -> u8 {
		self.z
	}
	pub fn as_geo(&self) -> [f64; 2] {
		let zoom: f64 = 2.0f64.powi(self.z as i32);

		[
			((self.x as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * (self.y as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		]
	}
	#[allow(dead_code)]
	pub fn as_geo_bbox(&self) -> [f64; 4] {
		let zoom: f64 = 2.0f64.powi(self.z as i32);

		[
			((self.x as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * (self.y as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
			(((self.x + 1) as f64) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * ((self.y + 1) as f64) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		]
	}
	pub fn as_coord2(&self) -> TileCoord2 {
		TileCoord2 { x: self.x, y: self.y }
	}
	pub fn as_json(&self) -> String {
		format!("{{x:{},y:{},z:{}}}", self.x, self.y, self.z)
	}
	pub fn is_valid(&self) -> bool {
		if self.z > 30 {
			return false;
		};
		let max = 2u32.pow(self.z as u32);
		(self.x < max) && (self.y < max)
	}
	pub fn get_sort_index(&self) -> u64 {
		let size = 2u64.pow(self.z as u32);
		let offset = (size * size - 1) / 3;
		offset + size * self.y as u64 + self.x as u64
	}
}

impl Debug for TileCoord3 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("TileCoord3({}, {}, {})", &self.x, &self.y, &self.z))
	}
}

impl PartialOrd for TileCoord3 {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		match self.z.partial_cmp(&other.z) {
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
			assert_eq!(TileCoord2::from_geo(xf, yf, z,), TileCoord2::new(x, y));
		};

		test(9, 267, 168, 8.0653, 52.2564);
		test(9, 273, 170, 12.3528, 51.3563);

		test(12, 1997, 1233, -4.43515, 58.0042);
		test(12, 2280, 1476, 20.4395, 44.8029);
	}

	#[test]
	fn debug() {
		assert_eq!(format!("{:?}", TileCoord2::new(1, 2)), "TileCoord2(1, 2)");
		assert_eq!(format!("{:?}", TileCoord3::new(1, 2, 3)), "TileCoord3(1, 2, 3)");
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
		let c = TileCoord3::new(2, 2, 2);
		assert!(c.eq(&c));
		assert!(c.eq(&c.clone()));
		assert!(c.ne(&TileCoord3::new(1, 2, 2)));
		assert!(c.ne(&TileCoord3::new(2, 1, 2)));
		assert!(c.ne(&TileCoord3::new(2, 2, 1)));
	}

	#[test]
	fn tilecoord2_new_and_getters() {
		let coord = TileCoord2::new(3, 4);
		assert_eq!(coord.get_x(), 3);
		assert_eq!(coord.get_y(), 4);
	}

	#[test]
	fn tilecoord2_substract() {
		let mut coord1 = TileCoord2::new(5, 7);
		let coord2 = TileCoord2::new(2, 3);
		coord1.substract(&coord2);
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
		let coord = TileCoord3::new(3, 4, 5);
		assert_eq!(coord.get_x(), 3);
		assert_eq!(coord.get_y(), 4);
		assert_eq!(coord.get_z(), 5);
	}

	#[test]
	fn tilecoord3_as_geo() {
		let coord = TileCoord3::new(3, 4, 5);
		assert_eq!(coord.as_geo(), [-146.25, 79.17133464081945]);
		assert_eq!(
			coord.as_geo_bbox(),
			[-146.25, 79.17133464081945, -135.0, 76.84081641443098]
		);
	}

	#[test]
	fn tilecoord3_as_coord2() {
		let coord = TileCoord3::new(3, 4, 5);
		let coord2 = coord.as_coord2();
		assert_eq!(coord2, TileCoord2::new(3, 4));
	}

	#[test]
	fn tilecoord3_is_valid() {
		let coord = TileCoord3::new(3, 4, 5);
		assert!(coord.is_valid());
	}

	#[test]
	fn tilecoord3_get_sort_index() {
		let coord = TileCoord3::new(3, 4, 5);
		assert_eq!(coord.get_sort_index(), 472);
	}

	#[test]
	fn hash() {
		let mut hasher = DefaultHasher::new();
		TileCoord2::new(2, 2).hash(&mut hasher);
		TileCoord3::new(2, 2, 2).hash(&mut hasher);
		assert_eq!(hasher.finish(), 8202047236025635059);
	}

	/*
	#[test]
	fn as_geo_bbox() {
		assert_eq!(
			TileCoord3::new(33, 22, 6).as_geo_bbox(),
			[5.625, 48.92249926375825, 11.25, 45.089035564831]
		);
	}
	*/

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

		let check = |x: u32, y: u32, z: u8, order: Ordering| {
			let c1 = TileCoord3::new(2, 2, 2);
			let c2 = TileCoord3::new(x, y, z);
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
