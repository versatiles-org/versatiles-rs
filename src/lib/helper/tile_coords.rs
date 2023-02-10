use std::{
	f32::consts::PI,
	f64::consts::PI as PI64,
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
	pub fn from_geo(x: f32, y: f32, z: u8) -> TileCoord2 {
		let zoom: f32 = 2.0f32.powi(z as i32);
		let x = zoom * (x / 360.0 + 0.5);
		let y = zoom * (0.5 - 0.5 * (y * PI / 360.0 + PI / 4.0).tan().ln() / PI);

		TileCoord2 {
			x: x as u64,
			y: y as u64,
		}
	}
	pub fn with_zoom(&self, z: u8) -> TileCoord3 {
		TileCoord3::new(self.x, self.y, z)
	}
}

impl fmt::Debug for TileCoord2 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("TileCoord2{{ x:{}, y:{} }}", &self.x, &self.y))
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
	pub fn to_geo(&self) -> [f64; 2] {
		let zoom: f64 = 2.0f64.powi(self.z as i32);

		[
			((self.x as f64) / zoom - 0.5) * 360.0,
			((PI64 * (1.0 - 2.0 * (self.y as f64) / zoom)).exp().atan() / PI64 - 0.25) * 360.0,
		]
	}
}

impl Debug for TileCoord3 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!(
			"TileCoord3{{ x:{}, y:{} z:{} }}",
			&self.x, &self.y, &self.z
		))
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

	#[test]
	fn from_geo() {
		let test = |z: u8, xf: f32, yf: f32, xi: u64, yi: u64| {
			let coord1 = TileCoord2::from_geo(xf, yf, z);
			let coord2 = TileCoord2::new(xi, yi);
			println!("coord1 {:?}", coord1);
			println!("coord2 {:?}", coord2);
			assert_eq!(coord1, coord2)
		};

		test(9, 8.0653, 52.2564, 267, 168);
		test(9, 12.3528, 51.3563, 273, 170);

		test(12, -4.43515, 58.0042, 1997, 1233);
		test(12, 20.4395, 44.8029, 2280, 1476);
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
}
