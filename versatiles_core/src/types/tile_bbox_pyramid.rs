//! This module defines the `TileBBoxPyramid` struct, which represents a pyramid of tile bounding boxes
//! across multiple zoom levels. It provides methods to create, manipulate, and query these bounding boxes.

use super::{GeoBBox, GeoCenter, TileBBox, TileCoord3};
use std::array::from_fn;
use std::fmt;

const MAX_ZOOM_LEVEL: u8 = 32;

/// A struct that represents a pyramid of tile bounding boxes across multiple zoom levels.
#[derive(Clone, Eq)]
pub struct TileBBoxPyramid {
	/// An array of tile bounding boxes, one for each zoom level up to `MAX_ZOOM_LEVEL`.
	pub level_bbox: [TileBBox; MAX_ZOOM_LEVEL as usize],
}

#[allow(dead_code)]
impl TileBBoxPyramid {
	/// Creates a new `TileBBoxPyramid` with full coverage up to the specified maximum zoom level.
	///
	/// # Arguments
	///
	/// * `max_zoom_level` - The maximum zoom level to be covered.
	///
	/// # Returns
	///
	/// A new `TileBBoxPyramid` instance.
	pub fn new_full(max_zoom_level: u8) -> TileBBoxPyramid {
		TileBBoxPyramid {
			level_bbox: from_fn(|z| {
				if z <= max_zoom_level as usize {
					TileBBox::new_full(z as u8).unwrap()
				} else {
					TileBBox::new_empty(z as u8).unwrap()
				}
			}),
		}
	}

	/// Creates a new `TileBBoxPyramid` with empty coverage for all zoom levels.
	///
	/// # Returns
	///
	/// A new `TileBBoxPyramid` instance.
	pub fn new_empty() -> TileBBoxPyramid {
		TileBBoxPyramid {
			level_bbox: from_fn(|z| TileBBox::new_empty(z as u8).unwrap()),
		}
	}

	/// Intersects the current pyramid with the specified geographical bounding box.
	///
	/// # Arguments
	///
	/// * `geo_bbox` - A reference to an array of four `f64` values representing the geographical bounding box.
	pub fn from_geo_bbox(zoom_level_min: u8, zoom_level_max: u8, bbox: &GeoBBox) -> TileBBoxPyramid {
		let mut pyramid = TileBBoxPyramid::new_empty();
		for z in zoom_level_min..=zoom_level_max {
			pyramid.set_level_bbox(TileBBox::from_geo(z, bbox).unwrap());
		}
		pyramid
	}

	/// Intersects the current pyramid with the specified geographical bounding box.
	///
	/// # Arguments
	///
	/// * `geo_bbox` - A reference to an array of four `f64` values representing the geographical bounding box.
	pub fn intersect_geo_bbox(&mut self, geo_bbox: &GeoBBox) {
		for (z, tile_bbox) in self.level_bbox.iter_mut().enumerate() {
			tile_bbox.intersect_bbox(&TileBBox::from_geo(z as u8, geo_bbox).unwrap());
		}
	}

	/// Adds a border to each level's bounding box.
	///
	/// # Arguments
	///
	/// * `x_min` - The minimum x-value of the border.
	/// * `y_min` - The minimum y-value of the border.
	/// * `x_max` - The maximum x-value of the border.
	/// * `y_max` - The maximum y-value of the border.
	pub fn add_border(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) {
		for bbox in self.level_bbox.iter_mut() {
			bbox.add_border(x_min, y_min, x_max, y_max);
		}
	}

	/// Intersects the current pyramid with another `TileBBoxPyramid`.
	///
	/// # Arguments
	///
	/// * `other_bbox_pyramid` - A reference to the other `TileBBoxPyramid`.
	pub fn intersect(&mut self, other_bbox_pyramid: &TileBBoxPyramid) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			let other_bbox = other_bbox_pyramid.get_level_bbox(level as u8);
			bbox.intersect_bbox(other_bbox);
		}
	}

	/// Returns a reference to the bounding box at the specified zoom level.
	///
	/// # Arguments
	///
	/// * `level` - The zoom level.
	///
	/// # Returns
	///
	/// A reference to the `TileBBox` at the specified level.
	pub fn get_level_bbox(&self, level: u8) -> &TileBBox {
		&self.level_bbox[level as usize]
	}

	/// Sets the bounding box at the specified level.
	///
	/// # Arguments
	///
	/// * `bbox` - The new bounding box to set.
	pub fn set_level_bbox(&mut self, bbox: TileBBox) {
		let level = bbox.level as usize;
		self.level_bbox[level] = bbox;
	}

	/// Includes a tile coordinate in the bounding box pyramid.
	///
	/// # Arguments
	///
	/// * `coord` - A reference to the `TileCoord3` to include.
	pub fn include_coord(&mut self, coord: &TileCoord3) {
		self.level_bbox[coord.z as usize].include_coord(coord.x, coord.y);
	}

	/// Includes a bounding box in the bounding box pyramid.
	///
	/// # Arguments
	///
	/// * `bbox` - A reference to the `TileBBox` to include.
	pub fn include_bbox(&mut self, bbox: &TileBBox) {
		self.level_bbox[bbox.level as usize].include_bbox(bbox);
	}

	pub fn include_bbox_pyramid(&mut self, pyramid: &TileBBoxPyramid) {
		for bbox in pyramid.iter_levels() {
			self.level_bbox[bbox.level as usize].include_bbox(bbox);
		}
	}

	pub fn contains_coord(&self, coord: &TileCoord3) -> bool {
		if let Some(bbox) = self.level_bbox.get(coord.z as usize) {
			bbox.contains3(coord)
		} else {
			false
		}
	}

	pub fn overlaps_bbox(&self, bbox: &TileBBox) -> bool {
		if let Some(bbox) = self.level_bbox.get(bbox.level as usize) {
			bbox.overlaps_bbox(bbox)
		} else {
			false
		}
	}

	/// Returns an iterator over the non-empty bounding boxes in the pyramid.
	///
	/// # Returns
	///
	/// An iterator over the non-empty `TileBBox` instances.
	pub fn iter_levels(&self) -> impl Iterator<Item = &TileBBox> {
		self.level_bbox.iter().filter(|bbox| !bbox.is_empty())
	}

	/// Returns the minimum zoom level that contains tiles.
	///
	/// # Returns
	///
	/// An `Option<u8>` representing the minimum zoom level, or `None` if all levels are empty.
	pub fn get_zoom_min(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.find(|bbox| !bbox.is_empty())
			.map(|bbox| bbox.level)
	}

	/// Returns the maximum zoom level that contains tiles.
	///
	/// # Returns
	///
	/// An `Option<u8>` representing the maximum zoom level, or `None` if all levels are empty.
	pub fn get_zoom_max(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.rev()
			.find(|bbox| !bbox.is_empty())
			.map(|bbox| bbox.level)
	}

	/// Returns a zoom level with a good number of tiles.
	///
	/// # Returns
	///
	/// An `Option<u8>` representing a good zoom level, or `None` if no levels contain more than 10 tiles.
	pub fn get_good_zoom(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.rev()
			.find(|bbox| bbox.count_tiles() > 10)
			.map(|bbox| bbox.level)
	}

	/// Sets the minimum zoom level, clearing any bounding boxes below this level.
	///
	/// # Arguments
	///
	/// * `zoom_level_min` - The minimum zoom level to set.
	pub fn set_zoom_min(&mut self, zoom_level_min: u8) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			if (index as u8) < zoom_level_min {
				bbox.set_empty();
			}
		}
	}

	/// Sets the maximum zoom level, clearing any bounding boxes above this level.
	///
	/// # Arguments
	///
	/// * `zoom_level_max` - The maximum zoom level to set.
	pub fn set_zoom_max(&mut self, zoom_level_max: u8) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			if (index as u8) > zoom_level_max {
				bbox.set_empty();
			}
		}
	}

	/// Counts the total number of tiles in the pyramid.
	///
	/// # Returns
	///
	/// The total number of tiles in the pyramid.
	pub fn count_tiles(&self) -> u64 {
		self.level_bbox.iter().map(|bbox| bbox.count_tiles()).sum()
	}

	/// Checks if the pyramid is empty.
	///
	/// # Returns
	///
	/// `true` if the pyramid is empty, `false` otherwise.
	pub fn is_empty(&self) -> bool {
		self.level_bbox.iter().all(|bbox| bbox.is_empty())
	}

	/// Checks if the pyramid is full up to the specified maximum zoom level.
	///
	/// # Arguments
	///
	/// * `max_zoom_level` - The maximum zoom level to check.
	///
	/// # Returns
	///
	/// `true` if the pyramid is full up to the specified zoom level, `false` otherwise.
	#[cfg(test)]
	pub fn is_full(&self, max_zoom_level: u8) -> bool {
		self.level_bbox.iter().all(|bbox| {
			if bbox.level <= max_zoom_level {
				bbox.is_full()
			} else {
				bbox.is_empty()
			}
		})
	}

	/// Returns the geographical bounding box of the pyramid.
	///
	/// # Returns
	///
	/// A four-element array of `f64` values representing the geographical bounding box.
	pub fn get_geo_bbox(&self) -> Option<GeoBBox> {
		let level = self.get_zoom_max()?;
		Some(self.get_level_bbox(level).as_geo_bbox())
	}

	pub fn get_geo_center(&self) -> Option<GeoCenter> {
		let bbox = self.get_geo_bbox()?;
		let zoom = (self.get_zoom_min()? + 2).min(self.get_zoom_max()?);
		Some(GeoCenter((bbox.0 + bbox.2) / 2.0, (bbox.1 + bbox.3) / 2.0, zoom))
	}
}

impl fmt::Debug for TileBBoxPyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entries(self.iter_levels()).finish()
	}
}

impl fmt::Display for TileBBoxPyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entries(self.iter_levels()).finish()
	}
}

impl PartialEq for TileBBoxPyramid {
	fn eq(&self, other: &Self) -> bool {
		for i in 0..MAX_ZOOM_LEVEL {
			let bbox0 = self.get_level_bbox(i);
			let bbox1 = other.get_level_bbox(i);
			if bbox0.is_empty() != bbox1.is_empty() {
				return false;
			}
			if bbox0.is_empty() {
				continue;
			}
			if bbox0 != bbox1 {
				return false;
			}
		}
		true
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn intersection_tests() {
		let mut pyramid1 = TileBBoxPyramid::new_empty();
		pyramid1.intersect(&TileBBoxPyramid::new_empty());
		assert!(pyramid1.is_empty());

		let mut pyramid1 = TileBBoxPyramid::new_full(8);
		pyramid1.intersect(&TileBBoxPyramid::new_empty());
		assert!(pyramid1.is_empty());

		let mut pyramid1 = TileBBoxPyramid::new_empty();
		pyramid1.intersect(&TileBBoxPyramid::new_full(8));
		assert!(pyramid1.is_empty());

		let mut pyramid1 = TileBBoxPyramid::new_full(8);
		pyramid1.intersect(&TileBBoxPyramid::new_full(8));
		assert!(pyramid1.is_full(8));
	}

	#[test]
	fn limit_by_geo_bbox() {
		let mut pyramid = TileBBoxPyramid::new_full(8);
		pyramid.intersect_geo_bbox(&GeoBBox(8.0653f64, 51.3563f64, 12.3528f64, 52.2564f64));

		assert_eq!(pyramid.get_level_bbox(0), &TileBBox::new(0, 0, 0, 0, 0).unwrap());
		assert_eq!(pyramid.get_level_bbox(1), &TileBBox::new(1, 1, 0, 1, 0).unwrap());
		assert_eq!(pyramid.get_level_bbox(2), &TileBBox::new(2, 2, 1, 2, 1).unwrap());
		assert_eq!(pyramid.get_level_bbox(3), &TileBBox::new(3, 4, 2, 4, 2).unwrap());
		assert_eq!(pyramid.get_level_bbox(4), &TileBBox::new(4, 8, 5, 8, 5).unwrap());
		assert_eq!(pyramid.get_level_bbox(5), &TileBBox::new(5, 16, 10, 17, 10).unwrap());
		assert_eq!(pyramid.get_level_bbox(6), &TileBBox::new(6, 33, 21, 34, 21).unwrap());
		assert_eq!(pyramid.get_level_bbox(7), &TileBBox::new(7, 66, 42, 68, 42).unwrap());
		assert_eq!(pyramid.get_level_bbox(8), &TileBBox::new(8, 133, 84, 136, 85).unwrap());
	}

	#[test]
	fn include_coord() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		pyramid.include_coord(&TileCoord3::new(1, 2, 3).unwrap());
		pyramid.include_coord(&TileCoord3::new(4, 5, 3).unwrap());
		pyramid.include_coord(&TileCoord3::new(6, 7, 8).unwrap());

		assert!(pyramid.get_level_bbox(0).is_empty());
		assert!(pyramid.get_level_bbox(1).is_empty());
		assert!(pyramid.get_level_bbox(2).is_empty());
		assert_eq!(pyramid.get_level_bbox(3), &TileBBox::new(3, 1, 2, 4, 5).unwrap());
		assert!(pyramid.get_level_bbox(4).is_empty());
		assert!(pyramid.get_level_bbox(5).is_empty());
		assert!(pyramid.get_level_bbox(6).is_empty());
		assert!(pyramid.get_level_bbox(7).is_empty());
		assert_eq!(pyramid.get_level_bbox(8), &TileBBox::new(8, 6, 7, 6, 7).unwrap());
		assert!(pyramid.get_level_bbox(9).is_empty());
	}

	#[test]
	fn include_bbox() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		pyramid.include_bbox(&TileBBox::new(4, 1, 2, 3, 4).unwrap());
		pyramid.include_bbox(&TileBBox::new(4, 5, 6, 7, 8).unwrap());

		assert!(pyramid.get_level_bbox(0).is_empty());
		assert!(pyramid.get_level_bbox(1).is_empty());
		assert!(pyramid.get_level_bbox(2).is_empty());
		assert!(pyramid.get_level_bbox(3).is_empty());
		assert_eq!(pyramid.get_level_bbox(4), &TileBBox::new(4, 1, 2, 7, 8).unwrap());
		assert!(pyramid.get_level_bbox(5).is_empty());
		assert!(pyramid.get_level_bbox(6).is_empty());
		assert!(pyramid.get_level_bbox(7).is_empty());
		assert!(pyramid.get_level_bbox(8).is_empty());
		assert!(pyramid.get_level_bbox(9).is_empty());
	}

	#[test]
	fn level_bbox() {
		let test = |level: u8| {
			let mut pyramid = TileBBoxPyramid::new_empty();
			let bbox = TileBBox::new_full(level).unwrap();
			pyramid.set_level_bbox(bbox.clone());
			assert_eq!(pyramid.get_level_bbox(level), &bbox);
		};

		test(0);
		test(1);
		test(30);
		test(31);
	}

	#[test]
	fn zoom_min_max() {
		let test = |z0: u8, z1: u8| {
			let mut pyramid = TileBBoxPyramid::new_full(z1);
			pyramid.set_zoom_min(z0);
			assert_eq!(pyramid.get_zoom_min().unwrap(), z0);
			assert_eq!(pyramid.get_zoom_max().unwrap(), z1);
		};

		test(0, 1);
		test(0, 30);
		test(30, 30);
	}

	#[test]
	fn add_border() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		pyramid.add_border(1, 2, 3, 4);
		assert!(pyramid.is_empty());

		let mut pyramid = TileBBoxPyramid::new_full(8);
		pyramid.intersect_geo_bbox(&GeoBBox(-9., -5., 5., 10.));
		pyramid.add_border(1, 2, 3, 4);

		// Check that each level's bounding box has been adjusted correctly.
		assert_eq!(pyramid.get_level_bbox(0), &TileBBox::new(0, 0, 0, 0, 0).unwrap());
		assert_eq!(pyramid.get_level_bbox(1), &TileBBox::new(1, 0, 0, 1, 1).unwrap());
		assert_eq!(pyramid.get_level_bbox(2), &TileBBox::new(2, 0, 0, 3, 3).unwrap());
		assert_eq!(pyramid.get_level_bbox(3), &TileBBox::new(3, 2, 1, 7, 7).unwrap());
		assert_eq!(pyramid.get_level_bbox(4), &TileBBox::new(4, 6, 5, 11, 12).unwrap());
		assert_eq!(pyramid.get_level_bbox(5), &TileBBox::new(5, 14, 13, 19, 20).unwrap());
		assert_eq!(pyramid.get_level_bbox(6), &TileBBox::new(6, 29, 28, 35, 36).unwrap());
		assert_eq!(pyramid.get_level_bbox(7), &TileBBox::new(7, 59, 58, 68, 69).unwrap());
		assert_eq!(
			pyramid.get_level_bbox(8),
			&TileBBox::new(8, 120, 118, 134, 135).unwrap()
		);
	}
}
