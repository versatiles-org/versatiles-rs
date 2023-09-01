use std::{
	f32::consts::PI as PI32,
	fmt::{self, Debug},
	mem::swap,
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
	pub fn from_geo(x: f32, y: f32, z: u8, round_ceil: bool) -> TileCoord2 {
		assert!(z <= 31, "z {z} must be <= 31");

		let zoom: f32 = 2.0f32.powi(z as i32);
		let x = zoom * (x / 360.0 + 0.5);
		let y = zoom * (0.5 - 0.5 * (y * PI32 / 360.0 + PI32 / 4.0).tan().ln() / PI32);

		if round_ceil {
			TileCoord2 {
				x: x.ceil() as u32,
				y: y.ceil() as u32,
			}
		} else {
			TileCoord2 {
				x: x as u32,
				y: y as u32,
			}
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
	x: u32,
	y: u32,
	z: u8,
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
	pub fn flip_y(mut self) -> Self {
		let max_index = 2u32.pow(self.z as u32) - 1;
		self.y = max_index - self.y;
		self
	}
	pub fn swap_xy(mut self) -> Self {
		swap(&mut self.x, &mut self.y);
		self
	}
	pub fn as_geo(&self) -> [f32; 2] {
		let zoom: f32 = 2.0f32.powi(self.z as i32);

		[
			((self.x as f32) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * (self.y as f32) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		]
	}
	pub fn as_coord2(&self) -> TileCoord2 {
		TileCoord2 { x: self.x, y: self.y }
	}
	#[cfg(test)]
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
		let test = |z: u8, x: u32, y: u32, xf: f32, yf: f32| {
			assert_eq!(TileCoord2::from_geo(xf, yf, z, false), TileCoord2::new(x, y));
			assert_eq!(TileCoord2::from_geo(xf, yf, z, true), TileCoord2::new(x + 1, y + 1));
		};

		test(9, 267, 168, 8.0653, 52.2564);
		test(9, 273, 170, 12.3528, 51.3563);

		test(12, 1997, 1233, -4.43515, 58.0042);
		test(12, 2280, 1476, 20.4395, 44.8029);
	}

	#[test]
	fn flip_y() {
		assert_eq!(TileCoord3::new(1, 2, 3).flip_y(), TileCoord3::new(1, 5, 3));
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
	fn hash() {
		let mut hasher = DefaultHasher::new();
		TileCoord2::new(2, 2).hash(&mut hasher);
		TileCoord3::new(2, 2, 2).hash(&mut hasher);
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
