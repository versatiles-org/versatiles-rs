//! Constructors for [`TilePyramid`].

use super::TilePyramid;
use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileCover, TileQuadtree};
use anyhow::Result;
use std::array::from_fn;

impl TilePyramid {
	/// Creates an empty pyramid (all levels empty).
	#[must_use]
	#[allow(clippy::cast_possible_truncation)]
	pub fn new_empty() -> Self {
		TilePyramid {
			levels: from_fn(|z| TileCover::new_empty(z as u8).unwrap()),
		}
	}

	/// Creates a full pyramid (all levels full).
	#[must_use]
	#[allow(clippy::cast_possible_truncation)]
	pub fn new_full() -> Self {
		TilePyramid {
			levels: from_fn(|z| TileCover::new_full(z as u8).unwrap()),
		}
	}

	/// Creates a full pyramid up to `max_zoom_level`; levels above are empty.
	#[must_use]
	#[allow(clippy::cast_possible_truncation)]
	pub fn new_full_up_to(max_zoom_level: u8) -> Self {
		TilePyramid {
			levels: from_fn(|z| {
				let level = z as u8;
				if level <= max_zoom_level {
					TileCover::new_full(level).unwrap()
				} else {
					TileCover::new_empty(level).unwrap()
				}
			}),
		}
	}

	/// Creates a pyramid from a geographic bounding box for the given zoom range.
	///
	/// # Errors
	/// Returns an error if any zoom level or geographic coordinate is invalid.
	pub fn from_geo_bbox(zoom_min: u8, zoom_max: u8, geo_bbox: &GeoBBox) -> Result<Self> {
		let mut pyramid = TilePyramid::new_empty();
		for z in zoom_min..=zoom_max {
			pyramid.levels[z as usize] = TileCover::from_geo(z, geo_bbox)?;
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
	fn default() -> Self {
		Self::new_empty()
	}
}

impl<T> From<&T> for TilePyramid
where
	T: ?Sized + AsRef<[TileBBox]>,
{
	fn from(bboxes: &T) -> Self {
		let mut pyramid = TilePyramid::new_empty();
		for bbox in bboxes.as_ref() {
			pyramid.include_bbox(bbox).expect("include_bbox failed");
		}
		pyramid
	}
}
