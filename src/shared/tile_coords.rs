use std::{
	f32::consts::PI as PI32,
	fmt::{self, Debug},
};

#[derive(Eq, PartialEq, Clone, Hash)]
pub struct TileCoord2 {
	pub x: u64,
	pub y: u64,
}
impl TileCoord2 {
	pub fn new(x: u64, y: u64) -> TileCoord2 {
		TileCoord2 { x, y }
	}
	pub fn from_geo(x: f32, y: f32, z: u8, round_ceil: bool) -> TileCoord2 {
		let zoom: f32 = 2.0f32.powi(z as i32);
		let x = zoom * (x / 360.0 + 0.5);
		let y = zoom * (0.5 - 0.5 * (y * PI32 / 360.0 + PI32 / 4.0).tan().ln() / PI32);

		if round_ceil {
			TileCoord2 {
				x: x.ceil() as u64,
				y: y.ceil() as u64,
			}
		} else {
			TileCoord2 {
				x: x as u64,
				y: y as u64,
			}
		}
	}
	pub fn with_zoom(&self, z: u8) -> TileCoord3 {
		TileCoord3::new(self.x, self.y, z)
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
	pub x: u64,
	pub y: u64,
	pub z: u8,
}
impl TileCoord3 {
	pub fn new(x: u64, y: u64, z: u8) -> TileCoord3 {
		TileCoord3 { x, y, z }
	}
	pub fn flip_vertically(&self) -> TileCoord3 {
		let max_index = 2u64.pow(self.z as u32) - 1;
		TileCoord3 {
			x: self.x,
			y: max_index - self.y,
			z: self.z,
		}
	}
	pub fn as_geo(&self) -> [f32; 2] {
		let zoom: f32 = 2.0f32.powi(self.z as i32);

		[
			((self.x as f32) / zoom - 0.5) * 360.0,
			((PI32 * (1.0 - 2.0 * (self.y as f32) / zoom)).exp().atan() / PI32 - 0.25) * 360.0,
		]
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
		let test = |z: u8, x: u64, y: u64, xf: f32, yf: f32| {
			assert_eq!(TileCoord2::from_geo(xf, yf, z, false), TileCoord2::new(x, y));
			assert_eq!(TileCoord2::from_geo(xf, yf, z, true), TileCoord2::new(x + 1, y + 1));
		};

		test(9, 267, 168, 8.0653, 52.2564);
		test(9, 273, 170, 12.3528, 51.3563);

		test(12, 1997, 1233, -4.43515, 58.0042);
		test(12, 2280, 1476, 20.4395, 44.8029);
	}

	#[test]
	fn with_zoom() {
		let coord = TileCoord2::new(1, 2);
		assert_eq!(coord.with_zoom(3), TileCoord3::new(1, 2, 3));
		assert_eq!(coord, TileCoord2::new(1, 2));
	}

	#[test]
	fn flip_vertically() {
		let coord = TileCoord3::new(1, 2, 3);
		assert_eq!(coord.flip_vertically(), TileCoord3::new(1, 5, 3));
		assert_eq!(coord, TileCoord3::new(1, 2, 3));
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
		assert_eq!(hasher.finish(), 8781784348340199787);
	}

	#[test]
	fn partial_cmp2() {
		use std::cmp::Ordering;
		use std::cmp::Ordering::*;

		let check = |x: u64, y: u64, order: Ordering| {
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

		let check = |x: u64, y: u64, z: u8, order: Ordering| {
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
