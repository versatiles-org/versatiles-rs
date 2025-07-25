use super::{TileBBox, TileCoord3};
use anyhow::{Result, bail};

pub trait HilbertIndex {
	fn get_hilbert_index(&self) -> Result<u64>;
	fn from_hilbert_index(index: u64) -> Result<Self>
	where
		Self: Sized;
}

impl HilbertIndex for TileBBox {
	fn get_hilbert_index(&self) -> Result<u64> {
		coord_to_index(self.x_min, self.y_min, self.level)
	}
	fn from_hilbert_index(index: u64) -> Result<Self> {
		let coord = index_to_coord(index)?;
		TileBBox::new(coord.z, coord.x, coord.y, coord.x, coord.y)
	}
}

impl HilbertIndex for TileCoord3 {
	fn get_hilbert_index(&self) -> Result<u64> {
		coord_to_index(self.x, self.y, self.z)
	}
	fn from_hilbert_index(index: u64) -> Result<Self> {
		index_to_coord(index)
	}
}

fn coord_to_index(x: u32, y: u32, z: u8) -> Result<u64> {
	if z >= 32 {
		bail!("tile zoom exceeds 64-bit limit");
	}

	let n = 1u32 << z;
	if x >= n || y >= n {
		bail!("tile x/y outside zoom level bounds");
	}

	let mut acc: i64 = 0;
	for t_z in 0..(z as i64) {
		acc += 1i64 << (t_z * 2)
	}

	let mut tx: i64 = x as i64;
	let mut ty: i64 = y as i64;
	let mut d: i64 = 0;
	let mut s: i64 = n as i64 / 2;
	while s > 0 {
		let rx: u8 = if (tx & s) > 0 { 1 } else { 0 };
		let ry: u8 = if (ty & s) > 0 { 1 } else { 0 };
		d += s * s * ((3 * rx) ^ ry) as i64;
		rotate(s, &mut tx, &mut ty, rx, ry);
		s /= 2;
	}

	Ok((acc + d) as u64)
}

fn rotate(s: i64, tx: &mut i64, ty: &mut i64, rx: u8, ry: u8) {
	if ry == 0 {
		if rx == 1 {
			*tx = s - 1 - *tx;
			*ty = s - 1 - *ty;
		}
		std::mem::swap(tx, ty);
	}
}

fn index_to_coord(index: u64) -> Result<TileCoord3> {
	let mut acc = 0;
	for t_z in 0..32 {
		let num_tiles = (1 << t_z) * (1 << t_z);
		if acc + num_tiles > index {
			let n = 1 << t_z;
			let mut t = index - acc;
			let mut tx: i64 = 0;
			let mut ty: i64 = 0;

			let mut s: i64 = 1;
			while s < n {
				let rx = ((t / 2) & 1) as u8;
				let ry = ((t ^ (rx as u64)) & 1) as u8;
				rotate(s, &mut tx, &mut ty, rx, ry);
				if rx == 1 {
					tx += s;
				}
				if ry == 1 {
					ty += s;
				}
				t /= 4;
				s *= 2;
			}

			return TileCoord3::new(tx as u32, ty as u32, t_z);
		}
		acc += num_tiles;
	}
	bail!("tile zoom exceeds 64-bit limit".to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_coord_to_tile_id_basic_inputs() -> Result<()> {
		assert_eq!(coord_to_index(1, 1, 1)?, 3);
		assert_eq!(coord_to_index(0, 0, 0)?, 0);
		assert_eq!(coord_to_index(2, 2, 2)?, 13);
		assert_eq!(coord_to_index(5, 3, 3)?, 73);
		assert_eq!(coord_to_index(7, 7, 3)?, 63);

		assert_eq!(coord_to_index(0, 0, 31)?, 1537228672809129301);
		assert_eq!(coord_to_index((1 << 31) - 1, (1 << 31) - 1, 31)?, 4611686018427387903);

		Ok(())
	}

	#[test]
	fn test_coord_to_tile_id_invalid_zoom() {
		assert_eq!(
			coord_to_index(1, 1, 32).unwrap_err().to_string(),
			"tile zoom exceeds 64-bit limit"
		);
	}

	#[test]
	fn test_coord_to_tile_id_out_of_bounds() {
		assert_eq!(
			coord_to_index(1, 0, 0).unwrap_err().to_string(),
			"tile x/y outside zoom level bounds"
		);
	}

	#[test]
	fn test_tile_id_to_coord() -> Result<()> {
		let mut f = 0f64;
		loop {
			let id0 = f as u64;
			let coord = index_to_coord(id0).unwrap();
			let id1 = coord_to_index(coord.x, coord.y, coord.z)?;
			assert_eq!(id0, id1);

			if coord.z > 30 {
				break;
			}
			f = f * 1.1 + 1.0;
		}
		Ok(())
	}
}
