use crate::{TileBBox, TileCoord};
use itertools::Itertools;

impl TileBBox {
	// -------------------------------------------------------------------------
	// Iteration Methods
	// -------------------------------------------------------------------------

	/// Returns an iterator over all tile coordinates within the bounding box.
	///
	/// The iteration is in row-major order.
	///
	/// # Returns
	///
	/// An iterator yielding `TileCoord` instances.
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
		let y_range = self.y_min()..=self.y_max();
		let x_range = self.x_min()..=self.x_max();
		y_range
			.cartesian_product(x_range)
			.map(|(y, x)| TileCoord::new(self.level, x, y).unwrap())
	}

	/// Consumes the bounding box and returns an iterator over all tile coordinates within it.
	///
	/// The iteration is in row-major order.
	///
	/// # Returns
	///
	/// An iterator yielding `TileCoord` instances.
	pub fn into_iter_coords(self) -> impl Iterator<Item = TileCoord> {
		let y_range = self.y_min()..=self.y_max();
		let x_range = self.x_min()..=self.x_max();
		y_range
			.cartesian_product(x_range)
			.map(move |(y, x)| TileCoord::new(self.level, x, y).unwrap())
	}

	/// Splits the bounding box into a grid of smaller bounding boxes of a specified size.
	///
	/// Each sub-bounding box will have dimensions at most `size x size` tiles.
	/// The last sub-bounding boxes in each row or column may be smaller if the original
	/// dimensions are not exact multiples of `size`.
	///
	/// # Arguments
	///
	/// * `size` - Maximum size of each grid cell.
	///
	/// # Returns
	///
	/// An iterator yielding `TileBBox` instances representing the grid.
	#[must_use]
	pub fn iter_bbox_grid(&self, size: u32) -> Box<dyn Iterator<Item = TileBBox> + '_> {
		assert!(size != 0, "size must be greater than 0");

		let level = self.level;
		let max = (1u32 << level) - 1;
		let mut meta_bbox = *self;
		meta_bbox.scale_down(size);

		let iter = meta_bbox
			.iter_coords()
			.map(move |coord| {
				let x = coord.x * size;
				let y = coord.y * size;

				let mut bbox =
					TileBBox::from_min_and_max(level, x, y, (x + size - 1).min(max), (y + size - 1).min(max)).unwrap();
				bbox.intersect_with(self).unwrap();
				bbox
			})
			.filter(|bbox| !bbox.is_empty())
			.collect::<Vec<TileBBox>>()
			.into_iter();

		Box::new(iter)
	}
}

#[cfg(test)]
mod tests {
	use std::{collections::HashMap, vec};

	use super::*;
	use anyhow::Result;
	use rstest::rstest;

	fn tc(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	// ------------------------------
	// iter_coords / into_iter_coords
	// ------------------------------
	#[test]
	fn iter_coords_row_major_and_count() -> Result<()> {
		// z=4, bbox: x=2..4 (3 cols), y=5..6 (2 rows) → 6 coords
		let bb = TileBBox::from_min_and_max(4, 2, 5, 4, 6)?;
		let v: Vec<_> = bb.iter_coords().collect();
		assert_eq!(v.len(), 6);
		// Row-major: y runs slowest? In this module we do y_range.cartesian_product(x_range)
		// which yields (y,x) pairs; with Itertools, cartesian_product iterates the
		// left iterator outer and right inner → for each y, iterate all x.
		let exp = vec![
			tc(4, 2, 5),
			tc(4, 3, 5),
			tc(4, 4, 5),
			tc(4, 2, 6),
			tc(4, 3, 6),
			tc(4, 4, 6),
		];
		assert_eq!(v, exp);
		Ok(())
	}

	#[test]
	fn into_iter_coords_consumes_and_matches() -> Result<()> {
		let bb = TileBBox::from_min_and_max(5, 10, 20, 11, 22)?; // 2 cols × 3 rows
		let a: Vec<_> = bb.iter_coords().collect();
		let b: Vec<_> = bb.into_iter_coords().collect();
		assert_eq!(a, b);
		assert_eq!(a.len(), 6);
		Ok(())
	}

	// ------------------------------
	// iter_bbox_grid
	// ------------------------------
	#[rstest]
	#[case::size2((5, 10, 20, 15, 25), 2, (3,3))]
	#[case::size4((6,  5,  6, 11,  9), 4, (2,2))]
	#[case::size8((7,  2,  3,  4,  4), 8, (1,1))]
	fn grid_cells_and_cover(
		#[case] minmax: (u8, u32, u32, u32, u32),
		#[case] size: u32,
		#[case] cells_xy: (usize, usize),
	) -> Result<()> {
		let (z, x0, y0, x1, y1) = minmax;
		let bb = TileBBox::from_min_and_max(z, x0, y0, x1, y1)?;
		let mut cols = HashMap::new();
		let mut rows = HashMap::new();
		for coord in bb.iter_bbox_grid(size) {
			cols.entry(coord.x_min()).and_modify(|c| *c += 1).or_insert(1);
			rows.entry(coord.y_min()).and_modify(|c| *c += 1).or_insert(1);
			assert!(bb.try_contains_bbox(&coord)?);
			assert!(coord.width() <= size);
			assert!(coord.height() <= size);
		}
		assert_eq!(cols.len(), cells_xy.0);
		assert_eq!(rows.len(), cells_xy.1);
		assert!(cols.values().all(|&c| c == cells_xy.1));
		assert!(rows.values().all(|&c| c == cells_xy.0));

		Ok(())
	}

	#[test]
	fn grid_first_last_cell_contents() -> Result<()> {
		// 5×4 bbox at z=6, grid size 2 → expect cells laid out left-to-right, top-to-bottom
		let bb = TileBBox::from_min_and_max(8, 100, 200, 104, 203)?;
		let mut it = bb.iter_bbox_grid(2);
		let first = it.next().unwrap();
		assert_eq!(
			(first.x_min(), first.y_min(), first.x_max(), first.y_max()),
			(100, 200, 101, 201)
		);
		// Exhaust and check last
		let last = it.last().unwrap();
		assert_eq!(
			(last.x_min(), last.y_min(), last.x_max(), last.y_max()),
			(104, 202, 104, 203)
		);
		Ok(())
	}

	#[test]
	#[should_panic(expected = "size must be greater than 0")]
	fn grid_panics_on_zero_size() {
		let bb = TileBBox::from_min_and_max(4, 0, 0, 3, 3).unwrap();
		let _ = bb.iter_bbox_grid(0).collect::<Vec<_>>();
	}
}
