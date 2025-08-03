use crate::{TileBBox, TileCoord3};
use anyhow::{Result, bail};

/// Converts between 2‑D tile space and its position along a **Hilbert
/// space‑filling curve**.
///
/// A Hilbert curve maps the tile grid at a given zoom level to a single
/// 64‑bit integer while largely preserving spatial locality.  
/// This is valuable for compact storage and range queries on tiled data.
///
/// The trait is implemented for:
/// * [`TileBBox`] – uses the south‑west corner of the bounding box.
/// * [`TileCoord3`] – a single `(z, x, y)` tile coordinate.
///
/// Implementors must guarantee that  
/// `Self::from_hilbert_index(idx)?.get_hilbert_index()? == idx`.
///
/// # Errors
/// Propagates conversion errors if the zoom level exceeds 31 or the
/// coordinates lie outside `[0, 2ᶻ − 1]`.
///
/// # Examples
/// ```
/// use versatiles_core::{TileCoord3, utils::HilbertIndex};
///
/// let coord = TileCoord3::new(5, 3, 3)?;
/// let idx = coord.get_hilbert_index()?;
/// assert_eq!(TileCoord3::from_hilbert_index(idx)?, coord);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub trait HilbertIndex {
	/// Returns the 64‑bit Hilbert index corresponding to `self`.
	fn get_hilbert_index(&self) -> Result<u64>;
	/// Reconstructs an instance from a 64‑bit Hilbert index produced by
	/// [`HilbertIndex::get_hilbert_index`].
	///
	/// # Errors
	/// Fails when `index` cannot be mapped to valid tile coordinates
	/// (e.g. if it would imply a zoom level ≥ 32).
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
		TileBBox::new(coord.level, coord.x, coord.y, coord.x, coord.y)
	}
}

impl HilbertIndex for TileCoord3 {
	fn get_hilbert_index(&self) -> Result<u64> {
		coord_to_index(self.x, self.y, self.level)
	}
	fn from_hilbert_index(index: u64) -> Result<Self> {
		index_to_coord(index)
	}
}

/// Encodes a Web‑Mercator tile coordinate `(x, y, z)` into its position
/// along the 64‑bit Hilbert space‑filling curve used by this crate.
///
/// * `x`, `y` – tile coordinates in the inclusive range `0..2ᶻ − 1`.
/// * `z` – zoom level (`0‒31`).  
///
/// Lower zoom levels occupy the lower portion of the 64‑bit range so that
/// indices remain strictly increasing with zoom.
///
/// # Errors
/// * **`"tile zoom exceeds 64-bit limit"`** – `z ≥ 32`
/// * **`"tile x/y outside zoom level bounds"`** – any coordinate ≥ `2ᶻ`
///
/// # Implementation notes
/// The function is a direct port of the canonical Hilbert algorithm that
/// traverses the curve iteratively while keeping an accumulator for the
/// number of tiles contained in all previous zoom levels.
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

#[doc(hidden)]
/// In‑place rotation/reflection helper for the Hilbert algorithm.
///
/// Given the quadrant bits `rx`/`ry`, it mutates the partial tile
/// coordinate `(tx, ty)` within a square of size `s × s` to follow the
/// Hilbert orientation rules described in Hamilton (1996).
///
/// Not part of the public API; exposed only for unit tests.
fn rotate(s: i64, tx: &mut i64, ty: &mut i64, rx: u8, ry: u8) {
	if ry == 0 {
		if rx == 1 {
			*tx = s - 1 - *tx;
			*ty = s - 1 - *ty;
		}
		std::mem::swap(tx, ty);
	}
}

/// Decodes a 64‑bit Hilbert index back into its `(x, y, z)` tile
/// coordinate.
///
/// This is the inverse of [`coord_to_index`]. The algorithm incrementally
/// subtracts the tile counts of successive zoom levels until it finds the
/// level that contains the given index.
///
/// # Errors
/// Returns **`"tile zoom exceeds 64-bit limit"`** when the index would
/// require a zoom level ≥ 32.
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

			return TileCoord3::new(t_z, tx as u32, ty as u32);
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
			let id1 = coord_to_index(coord.x, coord.y, coord.level)?;
			assert_eq!(id0, id1);

			if coord.level > 30 {
				break;
			}
			f = f * 1.1 + 1.0;
		}
		Ok(())
	}

	#[test]
	fn test_tile_id_to_coord_edge_cases() -> Result<()> {
		// Test the smallest possible index
		let coord = index_to_coord(0)?;
		assert_eq!(coord_to_index(coord.x, coord.y, coord.level)?, 0);

		// Test the largest possible index for zoom level 31
		let max_index = coord_to_index((1 << 31) - 1, (1 << 31) - 1, 31)?;
		let coord = index_to_coord(max_index)?;
		assert_eq!(coord_to_index(coord.x, coord.y, coord.level)?, max_index);

		Ok(())
	}

	#[test]
	fn test_tile_id_to_coord_invalid_index() {
		// Test an index that exceeds the 64-bit limit
		assert_eq!(
			index_to_coord(u64::MAX).unwrap_err().to_string(),
			"tile zoom exceeds 64-bit limit"
		);
	}

	#[test]
	fn test_hilbert_index_trait_tile_bbox() -> Result<()> {
		let bbox = TileBBox::new(3, 5, 3, 5, 3)?;
		let index = bbox.get_hilbert_index()?;
		let reconstructed_bbox = TileBBox::from_hilbert_index(index)?;
		assert_eq!(bbox, reconstructed_bbox);

		Ok(())
	}

	#[test]
	fn test_hilbert_index_trait_tile_coord3() -> Result<()> {
		let coord = TileCoord3::new(5, 3, 3)?;
		let index = coord.get_hilbert_index()?;
		let reconstructed_coord = TileCoord3::from_hilbert_index(index)?;
		assert_eq!(coord, reconstructed_coord);

		Ok(())
	}

	#[test]
	fn test_tile_id_to_coord_random() -> Result<()> {
		fn pseudo_random(r: &mut f64) -> f64 {
			*r = ((*r * 2000.0 + 0.2).sin() + 1.1) * 1000.0 % 1.0;
			*r
		}

		let mut r = 0.1;

		for z in 0..31 {
			let n = 1 << z;
			let x = (pseudo_random(&mut r) * n as f64) as u32;
			let y = (pseudo_random(&mut r) * n as f64) as u32;

			let coord = index_to_coord(coord_to_index(x, y, z)?)?;
			assert_eq!(coord.x, x);
			assert_eq!(coord.y, y);
			assert_eq!(coord.level, z);

			let coord = index_to_coord(coord_to_index(0, 0, z)?)?;
			assert_eq!(coord.x, 0);
			assert_eq!(coord.y, 0);
			assert_eq!(coord.level, z);

			let coord = index_to_coord(coord_to_index(n - 1, n - 1, z)?)?;
			assert_eq!(coord.x, n - 1);
			assert_eq!(coord.y, n - 1);
			assert_eq!(coord.level, z);
		}
		Ok(())
	}
}
