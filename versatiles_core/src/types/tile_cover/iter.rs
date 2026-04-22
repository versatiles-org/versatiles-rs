//! Iteration methods for [`TileCover`].

use super::TileCover;
use crate::{TileBBox, TileCoord};

impl TileCover {
	/// Returns an iterator over all tile coordinates in this cover.
	///
	/// - `Bbox` variant: iterates in row-major (raster scan) order.
	/// - `Tree` variant: iterates in depth-first quadtree order.
	#[must_use]
	pub fn iter_coords(&self) -> Box<dyn Iterator<Item = TileCoord> + '_> {
		match self {
			TileCover::Bbox(b) => Box::new(b.iter_coords()),
			TileCover::Tree(t) => Box::new(t.iter_coords()),
		}
	}

	/// Splits the covered area into a grid of `TileBBox` cells of the given `size`.
	///
	/// Uses [`to_bbox`](TileCover::to_bbox) to determine the area to partition.
	/// Returns an empty iterator if this cover is empty.
	pub fn iter_grid(&self, size: u32) -> impl Iterator<Item = TileBBox> + Send {
		let vec: Vec<TileBBox> = match self {
			TileCover::Bbox(b) => b.iter_grid(size).collect(),
			TileCover::Tree(t) => t
				.iter_grid(size)
				.map(|b| b.to_bbox())
				.filter(|b| !b.is_empty())
				.collect(),
		};
		vec.into_iter()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileQuadtree;
	use rstest::rstest;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	#[rstest]
	#[case(TileCover::new_empty(4).unwrap(), 0)]
	#[case(TileCover::from(bbox(3, 0, 0, 0, 0)), 1)] // single tile
	#[case(TileCover::from(bbox(3, 0, 0, 3, 3)), 16)] // 4x4
	#[case(TileCover::new_full(2).unwrap(), 16)] // full z=2
	#[case(TileCover::from(TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3))), 16)]
	fn iter_coords_count_matches_count_tiles(#[case] c: TileCover, #[case] expected: u64) {
		assert_eq!(c.iter_coords().count() as u64, expected);
		assert_eq!(c.count_tiles(), expected);
	}

	#[rstest]
	#[case(4, bbox(4, 0, 0, 7, 7), 4)] // 8×8, grid 4 → 2×2 = 4 cells
	#[case(4, bbox(4, 0, 0, 15, 15), 16)] // 16×16, grid 4 → 4×4 = 16
	#[case(8, bbox(4, 0, 0, 7, 7), 1)] // grid ≥ bbox → 1 cell
	#[case(4, TileBBox::new_empty(4).unwrap(), 0)]
	fn iter_grid_cell_counts(#[case] grid_size: u32, #[case] b: TileBBox, #[case] expected: usize) {
		let c = TileCover::from(b);
		assert_eq!(c.iter_grid(grid_size).count(), expected);
	}

	#[test]
	fn iter_coords_yields_expected_coordinates() {
		let c = TileCover::from(bbox(2, 1, 1, 2, 2));
		let coords: Vec<_> = c.iter_coords().collect();
		assert_eq!(coords.len(), 4);
		// Each coordinate must be at level 2 and inside the bbox.
		for coord in &coords {
			assert_eq!(coord.level, 2);
			assert!((1..=2).contains(&coord.x));
			assert!((1..=2).contains(&coord.y));
		}
	}
}
