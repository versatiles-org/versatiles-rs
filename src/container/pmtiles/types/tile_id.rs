use crate::types::{TileBBox, TileCoord3};

pub trait TileId {
	fn get_tile_id(&self) -> u64;
}

impl TileId for TileBBox {
	fn get_tile_id(&self) -> u64 {
		calc_id(self.x_min, self.y_min, self.level)
	}
}

impl TileId for TileCoord3 {
	fn get_tile_id(&self) -> u64 {
		calc_id(self.x, self.y, self.z)
	}
}

fn rotate(n: u64, x: &mut u64, y: &mut u64, rx: u64, ry: u64) {
	if ry == 0 {
		if rx == 1 {
			*x = n - 1 - *x;
			*y = n - 1 - *y;
		}

		std::mem::swap(x, y);
	}
}

fn calc_id(x: u32, y: u32, z: u8) -> u64 {
	if z >= 32 {
		panic!("tile zoom exceeds 64-bit limit");
	}

	let n = 1u32 << z;
	if x >= n || y >= n {
		panic!("tile x/y outside zoom level bounds");
	}

	let mut acc: u64 = 0;
	for t_z in 0..(z as u64) {
		acc += 1u64 << (t_z * 2)
	}

	let mut tx: u64 = x as u64;
	let mut ty: u64 = y as u64;
	let mut d: u64 = 0;
	let mut s: u64 = n as u64 / 2;
	while s > 0 {
		let rx: u64 = if (tx & s) > 0 { 1 } else { 0 };
		let ry: u64 = if (ty & s) > 0 { 1 } else { 0 };
		d += s * s * ((3 * rx) ^ ry);
		rotate(s, &mut tx, &mut ty, rx, ry);
		s /= 2;
	}

	acc + d
}
