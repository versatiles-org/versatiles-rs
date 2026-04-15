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
	pub fn iter_bbox_grid(&self, size: u32) -> impl Iterator<Item = TileBBox> + Send {
		let vec: Vec<TileBBox> = match self {
			TileCover::Bbox(b) => b.iter_bbox_grid(size).collect(),
			TileCover::Tree(t) => t.iter_bbox_grid(size).filter_map(|b| b.bbox()).collect(),
		};
		vec.into_iter()
	}
}
