//! Constructors for [`TilePyramid`].

use super::TilePyramid;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileCover, TileQuadtree};
use anyhow::Result;
use std::array::from_fn;

impl TilePyramid {
	/// Creates an empty pyramid (all levels empty).
	#[must_use]
	pub fn new_empty() -> Self {
		TilePyramid::from_fn(TileCover::new_empty)
	}

	/// Creates a full pyramid (all levels full).
	#[must_use]
	pub fn new_full() -> Self {
		TilePyramid::from_fn(TileCover::new_full)
	}

	/// Creates a full pyramid up to `max_level`; levels above are empty.
	#[must_use]
	pub fn new_full_up_to(max_level: u8) -> Self {
		TilePyramid::from_fn(|level| {
			if level <= max_level {
				TileCover::new_full(level)
			} else {
				TileCover::new_empty(level)
			}
		})
	}

	/// Constructs a pyramid by calling `f(level)` for each zoom level 0–`MAX_ZOOM_LEVEL`.
	pub fn from_fn(mut f: impl FnMut(u8) -> Result<TileCover>) -> Self {
		TilePyramid {
			levels: from_fn(|z| f(u8::try_from(z).unwrap()).unwrap()),
		}
	}

	/// Creates a pyramid from a geographic bounding box for the given zoom range.
	///
	/// # Errors
	/// Returns an error if any zoom level or geographic coordinate is invalid.
	pub fn from_geo_bbox(level_min: u8, level_max: u8, geo_bbox: &GeoBBox) -> Result<Self> {
		let mut pyramid = TilePyramid::new_empty();
		for z in level_min..=level_max {
			pyramid.levels[z as usize] = TileCover::from_geo_bbox(z, geo_bbox)?;
		}
		Ok(pyramid)
	}

	/// Build a pyramid from an iterator of [`TileCoord`]s, one per tile.
	///
	/// Internally groups coordinates by zoom level and calls
	/// [`TileQuadtree::from_tile_coords`] for each non-empty level, giving
	/// O(T log T + T · level) overall instead of O(T · level²) for sequential
	/// insertion.
	#[must_use]
	#[allow(clippy::cast_possible_truncation)]
	pub fn from_tile_coords(coords: impl Iterator<Item = TileCoord>) -> Self {
		let mut per_level: Vec<Vec<(u32, u32)>> = vec![Vec::new(); (MAX_ZOOM_LEVEL + 1) as usize];
		for c in coords {
			per_level[c.level as usize].push((c.x, c.y));
		}
		let mut pyramid = TilePyramid::new_empty();
		for (z, tiles) in per_level.into_iter().enumerate() {
			if !tiles.is_empty() {
				let tree = TileQuadtree::from_tile_coords(z as u8, &tiles).expect("zoom level already validated");
				pyramid.levels[z] = TileCover::Tree(tree);
			}
		}
		pyramid
	}
}

impl Default for TilePyramid {
	/// Returns an empty pyramid (equivalent to [`TilePyramid::new_empty`]).
	fn default() -> Self {
		Self::new_empty()
	}
}

impl<T> From<&T> for TilePyramid
where
	T: ?Sized + AsRef<[TileBBox]>,
{
	/// Builds a pyramid by inserting each bbox in the slice at its zoom level.
	fn from(bboxes: &T) -> Self {
		let mut pyramid = TilePyramid::new_empty();
		for bbox in bboxes.as_ref() {
			pyramid.insert_bbox(bbox).expect("include_bbox failed");
		}
		pyramid
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::MAX_LAT;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn new_empty() {
		let p = TilePyramid::new_empty();
		assert!(p.is_empty());
		assert_eq!(p.level_min(), None);
		assert_eq!(p.level_max(), None);
		assert_eq!(p.count_tiles(), 0);
	}

	#[test]
	fn new_full() {
		let p = TilePyramid::new_full();
		assert!(!p.is_empty());
		assert_eq!(p.level_min(), Some(0));
		assert_eq!(p.level_max(), Some(30));
	}

	#[test]
	fn new_full_up_to() {
		let p = TilePyramid::new_full_up_to(5);
		assert_eq!(p.level_min(), Some(0));
		assert_eq!(p.level_max(), Some(5));
		assert!(p.level_ref(6).is_empty());
	}

	#[test]
	fn default_is_empty() {
		assert!(TilePyramid::default().is_empty());
	}

	#[test]
	fn from_geo_bbox() {
		let geo = GeoBBox::new(-180.0, -MAX_LAT, 180.0, MAX_LAT).unwrap();
		let p = TilePyramid::from_geo_bbox(0, 3, &geo).unwrap();
		assert_eq!(p.level_min(), Some(0));
		assert_eq!(p.level_max(), Some(3));
		assert!(p.level_ref(4).is_empty());
	}

	#[test]
	fn from_slice_of_bboxes() {
		let bboxes = vec![bbox(3, 0, 0, 3, 3), bbox(5, 1, 1, 5, 5)];
		let p = TilePyramid::from(bboxes.as_slice());
		assert_eq!(p.level_bbox(3), bbox(3, 0, 0, 3, 3));
		assert_eq!(p.level_bbox(5), bbox(5, 1, 1, 5, 5));
	}

	#[test]
	fn from_tile_coords_empty() {
		let p = TilePyramid::from_tile_coords(std::iter::empty());
		assert!(p.is_empty());
	}

	#[test]
	fn from_tile_coords_single_tile() {
		let c = TileCoord::new(5, 10, 12).unwrap();
		let p = TilePyramid::from_tile_coords(std::iter::once(c));
		assert!(!p.is_empty());
		assert_eq!(p.level_min(), Some(5));
		assert_eq!(p.level_max(), Some(5));
		assert_eq!(p.count_tiles(), 1);
	}

	#[test]
	fn from_tile_coords_multi_level() {
		// Tiles at zoom 2 and zoom 4
		let coords = vec![
			TileCoord::new(2, 1, 1).unwrap(),
			TileCoord::new(2, 2, 2).unwrap(),
			TileCoord::new(4, 5, 7).unwrap(),
		];
		let p = TilePyramid::from_tile_coords(coords.into_iter());
		assert_eq!(p.level_min(), Some(2));
		assert_eq!(p.level_max(), Some(4));
		assert_eq!(p.count_tiles(), 3);
	}

	#[test]
	fn from_tile_coords_matches_include_coord() {
		let coords = vec![
			TileCoord::new(3, 0, 0).unwrap(),
			TileCoord::new(3, 1, 2).unwrap(),
			TileCoord::new(3, 5, 6).unwrap(),
		];

		// Build via from_tile_coords
		let batch = TilePyramid::from_tile_coords(coords.clone().into_iter());

		// Build via sequential include_coord
		let mut seq = TilePyramid::new_empty();
		for c in &coords {
			seq.insert_coord(c);
		}

		assert_eq!(batch.count_tiles(), seq.count_tiles());
	}
}
