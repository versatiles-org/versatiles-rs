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
	/// Uses [`bbox`](TileCover::bbox) to determine the area to partition.
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

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn iter_tiles_count() {
		let c = TileCover::from(bbox(3, 0, 0, 3, 3));
		assert_eq!(c.iter_coords().count(), 16);
	}

	#[test]
	fn iter_grid_empty() {
		let c = TileCover::new_empty(4).unwrap();
		assert_eq!(c.iter_grid(4).count(), 0);
	}

	#[test]
	fn iter_grid_nonempty() {
		let c = TileCover::from(bbox(4, 0, 0, 7, 7));
		// 8×8 tiles split into 4×4 blocks → 4 blocks
		assert_eq!(c.iter_grid(4).count(), 4);
	}
}
