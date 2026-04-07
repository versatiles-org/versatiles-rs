//! Pyramid of tile bounding boxes across multiple zoom levels.
//!
//! A `TileBBoxPyramid` holds one [`TileBBox`] per zoom level (0 through
//! `MAX_ZOOM_LEVEL`), forming a hierarchical description of tile coverage.
//! Methods on this struct allow creation, manipulation, and querying of
//! coverage across all levels simultaneously.

use crate::{GeoBBox, MAX_ZOOM_LEVEL, TileBBox};
use std::array::from_fn;

/// A struct that represents a pyramid of tile bounding boxes across multiple zoom levels.
///
/// Each level (`0` through `MAX_ZOOM_LEVEL-1`) corresponds to a [`TileBBox`], which captures
/// the range of tile coordinates valid for that zoom level. Methods in this struct allow
/// you to intersect these bounding boxes with geographical extents, combine them with other
/// bounding boxes or pyramids, and query the pyramid for relevant information.
#[derive(Clone, Eq)]
pub struct TileBBoxPyramid {
	/// An array of tile bounding boxes, one for each zoom level up to `MAX_ZOOM_LEVEL`.
	///
	/// Levels beyond your area of interest might remain empty.
	pub level_bbox: [TileBBox; (MAX_ZOOM_LEVEL + 1) as usize],
}

impl TileBBoxPyramid {
	/// Creates a new `TileBBoxPyramid` with "full coverage" up to the specified `max_zoom_level`.
	///
	/// Higher levels (beyond `max_zoom_level`) remain empty.
	///
	/// # Arguments
	///
	/// * `max_zoom_level` - The maximum zoom level to be covered with a "full" bounding box.
	///
	/// # Returns
	///
	/// A `TileBBoxPyramid` where levels `0..=max_zoom_level` each have a full bounding box,
	/// and levels above that are empty.
	///
	/// # Panics
	///
	/// May panic if `max_zoom_level` exceeds `MAX_ZOOM_LEVEL - 1`.
	#[must_use]
	pub fn new_full_up_to(max_zoom_level: u8) -> TileBBoxPyramid {
		// Create an array of tile bounding boxes via `from_fn`.
		// If index <= max_zoom_level, create a full bounding box;
		// otherwise, create an empty bounding box.
		TileBBoxPyramid {
			level_bbox: from_fn(|z| {
				let level = u8::try_from(z).expect("zoom level index exceeds u8::MAX");
				if z <= max_zoom_level as usize {
					TileBBox::new_full(level).unwrap()
				} else {
					TileBBox::new_empty(level).unwrap()
				}
			}),
		}
	}

	/// Creates a new `TileBBoxPyramid` with "full coverage" for **all** zoom levels.
	///
	/// This is equivalent to `new_full_up_to(MAX_ZOOM_LEVEL)`.
	///
	/// # Returns
	///
	/// A `TileBBoxPyramid` where every level has a full bounding box.
	#[must_use]
	pub fn new_full() -> TileBBoxPyramid {
		TileBBoxPyramid::new_full_up_to(MAX_ZOOM_LEVEL)
	}

	/// Creates a new `TileBBoxPyramid` with empty coverage for **all** zoom levels.
	///
	/// # Returns
	///
	/// A `TileBBoxPyramid` where each level is an empty bounding box.
	#[must_use]
	pub fn new_empty() -> TileBBoxPyramid {
		TileBBoxPyramid {
			level_bbox: from_fn(|z| {
				TileBBox::new_empty(u8::try_from(z).expect("zoom level index exceeds u8::MAX")).unwrap()
			}),
		}
	}

	/// Constructs a new `TileBBoxPyramid` by intersecting a provided [`GeoBBox`]
	/// with each zoom level in the range `[zoom_level_min..=zoom_level_max]`.
	///
	/// # Arguments
	///
	/// * `zoom_level_min` - The smallest zoom level to include.
	/// * `zoom_level_max` - The largest zoom level to include.
	/// * `bbox` - The geographical bounding box to intersect with.
	///
	/// # Returns
	///
	/// A new `TileBBoxPyramid` populated with bounding boxes derived from `bbox`.
	/// Levels outside the given range remain empty.
	#[must_use]
	pub fn from_geo_bbox(zoom_level_min: u8, zoom_level_max: u8, bbox: &GeoBBox) -> TileBBoxPyramid {
		let mut pyramid = TileBBoxPyramid::new_empty();
		for z in zoom_level_min..=zoom_level_max {
			pyramid.set_level_bbox(TileBBox::from_geo(z, bbox).unwrap());
		}
		pyramid
	}
}

impl Default for TileBBoxPyramid {
	/// Creates a new `TileBBoxPyramid` with all levels empty.
	fn default() -> Self {
		Self::new_empty()
	}
}

impl<T> From<&T> for TileBBoxPyramid
where
	T: ?Sized + AsRef<[TileBBox]>,
{
	fn from(bboxes: &T) -> Self {
		let mut pyramid = TileBBoxPyramid::new_empty();
		for bbox in bboxes.as_ref() {
			pyramid.include_bbox(bbox);
		}
		pyramid
	}
}
