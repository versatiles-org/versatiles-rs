//! The [`PyramidInfo`] trait provides a common interface for querying
//! geographic and zoom-level metadata from pyramid types.
//!
//! Both [`TileBBoxPyramid`](crate::TileBBoxPyramid) and
//! [`TileQuadtreePyramid`](crate::TileQuadtreePyramid) implement this trait,
//! allowing code such as [`TileJSON::update_from_pyramid`](crate::TileJSON) to
//! accept either type without modification.

use crate::GeoBBox;

/// A trait for pyramid types that can report their geographic extent and zoom range.
///
/// Implement this trait to allow a pyramid type to be used with
/// [`TileJSON::update_from_pyramid`](crate::TileJSON).
pub trait PyramidInfo {
	/// Returns the geographic bounding box covering all tiles in the pyramid,
	/// or `None` if the pyramid is empty.
	fn get_geo_bbox(&self) -> Option<GeoBBox>;

	/// Returns the minimum (lowest) non-empty zoom level, or `None` if empty.
	fn get_zoom_min(&self) -> Option<u8>;

	/// Returns the maximum (highest) non-empty zoom level, or `None` if empty.
	fn get_zoom_max(&self) -> Option<u8>;
}
