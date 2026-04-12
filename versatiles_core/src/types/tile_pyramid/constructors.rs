//! Constructors for [`TilePyramid`].

use super::TilePyramid;
use crate::{GeoBBox, TileBBox, TileBBoxPyramid, TileCover, TileQuadtreePyramid};
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

	/// Converts a [`TileBBoxPyramid`] into a `TilePyramid` (each level becomes
	/// a `TileCover::Bbox`).
	#[must_use]
	#[allow(clippy::cast_possible_truncation)]
	pub fn from_bbox_pyramid(pyramid: &TileBBoxPyramid) -> Self {
		TilePyramid {
			levels: from_fn(|z| TileCover::from(*pyramid.get_level_bbox(z as u8))),
		}
	}

	/// Converts a [`TileQuadtreePyramid`] into a `TilePyramid` (each level
	/// becomes a `TileCover::Tree`).
	#[must_use]
	#[allow(clippy::cast_possible_truncation)]
	pub fn from_quadtree_pyramid(pyramid: &TileQuadtreePyramid) -> Self {
		TilePyramid {
			levels: from_fn(|z| TileCover::from(pyramid.get_level(z as u8).clone())),
		}
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
