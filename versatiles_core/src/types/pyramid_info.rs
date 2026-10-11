//! The [`PyramidInfo`] trait provides a common interface for querying
//! geographic and zoom-level metadata from pyramid types.
//!
//! [`TilePyramid`](crate::TilePyramid) implements this trait,
//! allowing code such as [`TileJSON::update_from_pyramid`](crate::TileJSON) to
//! accept it generically.

use crate::GeoBBox;

/// A trait for pyramid types that can report their geographic extent and zoom range.
///
/// Implement this trait to allow a pyramid type to be used with
/// [`TileJSON::update_from_pyramid`](crate::TileJSON).
pub trait PyramidInfo {
	/// Returns the geographic bounding box of the data in the pyramid, taking
	/// every level into account, or `None` if the pyramid is empty.
	fn geo_extent(&self) -> Option<GeoBBox>;

	/// The name [`geo_extent`](Self::geo_extent) had up to 5.0.
	///
	/// Kept so that code calling it through this trait still compiles after a
	/// routine `cargo update`. A type that implements the trait has to
	/// implement `geo_extent` instead.
	#[deprecated(since = "5.1.0", note = "renamed to `geo_extent`")]
	fn geo_bbox(&self) -> Option<GeoBBox> {
		self.geo_extent()
	}

	/// Returns the minimum (lowest) non-empty zoom level, or `None` if empty.
	fn level_min(&self) -> Option<u8>;

	/// Returns the maximum (highest) non-empty zoom level, or `None` if empty.
	fn level_max(&self) -> Option<u8>;
}

#[cfg(test)]
mod tests {
	use super::*;

	struct OneTile;

	impl PyramidInfo for OneTile {
		fn geo_extent(&self) -> Option<GeoBBox> {
			GeoBBox::new(-10.0, -5.0, 10.0, 5.0).ok()
		}

		fn level_min(&self) -> Option<u8> {
			Some(0)
		}

		fn level_max(&self) -> Option<u8> {
			Some(0)
		}
	}

	#[test]
	#[expect(deprecated, reason = "the alias is what is being tested")]
	fn the_old_name_still_answers() {
		assert_eq!(OneTile.geo_bbox(), OneTile.geo_extent());
	}
}
