use std::{
	f32::consts::PI,
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
	pub fn from_geo(level: u64, x: f32, y: f32) -> TileCoord2 {
		let zoom: f32 = 2.0f32.powi(level as i32);
		let x = zoom * (x / 360.0 + 0.5);
		let y = zoom * (0.5 - 0.5 * (y * PI / 360.0 + PI / 4.0).tan().ln() / PI);

		TileCoord2 {
			x: x as u64,
			y: y as u64,
		}
	}
}

impl fmt::Debug for TileCoord2 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!("TileCoord2{{ x:{}, y:{} }}", &self.x, &self.y))
	}
}

#[derive(Eq, PartialEq, Clone, Hash)]
pub struct TileCoord3 {
	pub x: u64,
	pub y: u64,
	pub z: u64,
}
impl TileCoord3 {
	pub fn new(z: u64, y: u64, x: u64) -> TileCoord3 {
		TileCoord3 { x, y, z }
	}
}

impl Debug for TileCoord3 {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!(
			"TileCoord3{{ z:{}, y:{} x:{} }}",
			&self.z, &self.y, &self.x
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
		let test = |level: u64, xf: f32, yf: f32, xi: u64, yi: u64| {
			let coord1 = TileCoord2::from_geo(level, xf, yf);
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
}
