use crate::{TileBBox, TileCoord};
use itertools::Itertools;

impl TileBBox {
	// -------------------------------------------------------------------------
	// Iteration Methods
	// -------------------------------------------------------------------------

	/// Returns an iterator over all tile coordinates within the bounding box in row-major order.
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
		if self.is_empty() {
			return Box::new(std::iter::empty()) as Box<dyn Iterator<Item = TileCoord>>;
		}
		let y_range = self.y_min().expect("bbox is non-empty")..=self.y_max().expect("bbox is non-empty");
		let x_range = self.x_min().expect("bbox is non-empty")..=self.x_max().expect("bbox is non-empty");
		Box::new(
			y_range
				.cartesian_product(x_range)
				.map(|(y, x)| TileCoord::new(self.level, x, y).expect("coord within bbox")),
		) as Box<dyn Iterator<Item = TileCoord>>
	}

	/// Consumes the bounding box and returns a `Send` iterator over all tile coordinates in
	/// row-major order.  Prefer [`iter_coords`](Self::iter_coords) when ownership is not needed.
	#[must_use]
	pub fn into_iter_coords(self) -> Box<dyn Iterator<Item = TileCoord> + Send> {
		if self.is_empty() {
			return Box::new(std::iter::empty());
		}
		let y_range = self.y_min().expect("bbox is non-empty")..=self.y_max().expect("bbox is non-empty");
		let x_range = self.x_min().expect("bbox is non-empty")..=self.x_max().expect("bbox is non-empty");
		let level = self.level;
		Box::new(
			y_range
				.cartesian_product(x_range)
				.map(move |(y, x)| TileCoord::new(level, x, y).expect("coord within bbox")),
		)
	}

	/// Splits the bounding box into an aligned grid of smaller bounding boxes.
	///
	/// `size` must be a power of two.
	/// Each cell covers at most `size × size`.
	/// Empty cells are omitted.
	#[must_use]
	pub fn iter_grid(&self, size: u32) -> Box<dyn Iterator<Item = TileBBox> + '_> {
		assert!(size.is_power_of_two(), "size must be a power of two");

		let level = self.level;
		let max = (1u32 << level) - 1;
		let mut meta_bbox = *self;
		meta_bbox.scale_down(size);

		Box::new(
			meta_bbox
				.into_iter_coords()
				.map(move |coord| {
					let x = coord.x * size;
					let y = coord.y * size;

					let mut bbox = TileBBox::from_min_and_max(level, x, y, (x + size - 1).min(max), (y + size - 1).min(max))
						.expect("grid cell within level bounds");
					bbox.intersect_bbox(self).expect("same-level intersection");
					bbox
				})
				.filter(|bbox| !bbox.is_empty()),
		)
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
	// iter_grid
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
		for coord in bb.iter_grid(size) {
			cols.entry(coord.x_min()?).and_modify(|c| *c += 1).or_insert(1);
			rows.entry(coord.y_min()?).and_modify(|c| *c += 1).or_insert(1);
			assert!(bb.includes_bbox(&coord));
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
		let mut it = bb.iter_grid(2);
		let first = it.next().unwrap();
		assert_eq!(
			(first.x_min()?, first.y_min()?, first.x_max()?, first.y_max()?),
			(100, 200, 101, 201)
		);
		// Exhaust and check last
		let last = it.last().unwrap();
		assert_eq!(
			(last.x_min()?, last.y_min()?, last.x_max()?, last.y_max()?),
			(104, 202, 104, 203)
		);
		Ok(())
	}

	#[test]
	#[should_panic(expected = "size must be a power of two")]
	fn grid_panics_on_non_power_of_two() {
		let bb = TileBBox::from_min_and_max(4, 0, 0, 3, 3).unwrap();
		let _ = bb.iter_grid(3).collect::<Vec<_>>();
	}

	#[test]
	fn iter_coords() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(16, 1, 5, 2, 6)?;
		let v: Vec<TileCoord> = bbox.iter_coords().collect();
		assert_eq!(v.len(), 4);
		assert_eq!(v[0], tc(16, 1, 5));
		assert_eq!(v[1], tc(16, 2, 5));
		assert_eq!(v[2], tc(16, 1, 6));
		assert_eq!(v[3], tc(16, 2, 6));
		Ok(())
	}

	#[rstest]
	#[case(16, (10, 0, 0, 31, 31), "0,0,15,15 16,0,31,15 0,16,15,31 16,16,31,31")]
	#[case(16, (10, 5, 6, 25, 26), "5,6,15,15 16,6,25,15 5,16,15,26 16,16,25,26")]
	#[case(16, (10, 5, 6, 16, 16), "5,6,15,15 16,6,16,15 5,16,15,16 16,16,16,16")]
	#[case(16, (10, 5, 6, 16, 15), "5,6,15,15 16,6,16,15")]
	#[case(16, (10, 6, 7, 6, 7), "6,7,6,7")]
	#[case(64, (4, 6, 7, 6, 7), "6,7,6,7")]
	fn iter_grid_cases(#[case] size: u32, #[case] def: (u8, u32, u32, u32, u32), #[case] expected: &str) -> Result<()> {
		let bbox = TileBBox::from_min_and_max(def.0, def.1, def.2, def.3, def.4)?;
		let result: String = bbox
			.iter_grid(size)
			.map(|bbox| {
				format!(
					"{},{},{},{}",
					bbox.x_min().unwrap(),
					bbox.y_min().unwrap(),
					bbox.x_max().unwrap(),
					bbox.y_max().unwrap()
				)
			})
			.collect::<Vec<String>>()
			.join(" ");
		assert_eq!(result, expected);
		Ok(())
	}

	#[test]
	fn should_iterate_over_coords_correctly() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 6, 11)?;
		let coords: Vec<TileCoord> = bbox.iter_coords().collect();
		let expected_coords = vec![tc(4, 5, 10), tc(4, 6, 10), tc(4, 5, 11), tc(4, 6, 11)];
		assert_eq!(coords, expected_coords);
		Ok(())
	}

	#[test]
	fn should_iterate_over_coords_correctly_when_consumed() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 6, 11)?;
		let coords: Vec<TileCoord> = bbox.into_iter_coords().collect();
		let expected_coords = vec![tc(4, 5, 10), tc(4, 6, 10), tc(4, 5, 11), tc(4, 6, 11)];
		assert_eq!(coords, expected_coords);
		Ok(())
	}

	#[test]
	fn should_split_bbox_into_correct_grid() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 0, 0, 7, 7)?;

		let grid_size = 4;
		let grids: Vec<TileBBox> = bbox.iter_grid(grid_size).collect();

		let expected_grids = vec![
			TileBBox::from_min_and_max(4, 0, 0, 3, 3)?,
			TileBBox::from_min_and_max(4, 4, 0, 7, 3)?,
			TileBBox::from_min_and_max(4, 0, 4, 3, 7)?,
			TileBBox::from_min_and_max(4, 4, 4, 7, 7)?,
		];

		assert_eq!(grids, expected_grids);

		Ok(())
	}

	#[test]
	fn should_handle_empty_bbox_in_grid_iteration() -> Result<()> {
		let bbox = TileBBox::new_empty(4)?;
		let grids: Vec<TileBBox> = bbox.iter_grid(4).collect();
		assert!(grids.is_empty());
		Ok(())
	}

	#[test]
	fn should_handle_single_tile_in_grid_iteration() -> Result<()> {
		let bbox = TileBBox::from_min_and_max(4, 5, 10, 5, 10)?;
		let grids: Vec<TileBBox> = bbox.iter_grid(4).collect();
		let expected_grids = vec![TileBBox::from_min_and_max(4, 5, 10, 5, 10)?];
		assert_eq!(grids, expected_grids);
		Ok(())
	}
}
