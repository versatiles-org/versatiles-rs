//! This module defines the `TileBBoxPyramid` struct, which represents a pyramid of tile bounding boxes
//! across multiple zoom levels. It provides methods to create, manipulate, and query these bounding boxes.

use super::{GeoBBox, GeoCenter, TileBBox, TileCoord3};
use std::array::from_fn;
use std::fmt;

const MAX_ZOOM_LEVEL: u8 = 32;

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
	pub level_bbox: [TileBBox; MAX_ZOOM_LEVEL as usize],
}

#[allow(dead_code)]
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
	pub fn new_full(max_zoom_level: u8) -> TileBBoxPyramid {
		// Create an array of tile bounding boxes via `from_fn`.
		// If index <= max_zoom_level, create a full bounding box;
		// otherwise, create an empty bounding box.
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

	/// Creates a new `TileBBoxPyramid` with empty coverage for **all** zoom levels.
	///
	/// # Returns
	///
	/// A `TileBBoxPyramid` where each level is an empty bounding box.
	pub fn new_empty() -> TileBBoxPyramid {
		TileBBoxPyramid {
			level_bbox: from_fn(|z| TileBBox::new_empty(z as u8).unwrap()),
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
	pub fn from_geo_bbox(zoom_level_min: u8, zoom_level_max: u8, bbox: &GeoBBox) -> TileBBoxPyramid {
		let mut pyramid = TileBBoxPyramid::new_empty();
		for z in zoom_level_min..=zoom_level_max {
			pyramid.set_level_bbox(TileBBox::from_geo(z, bbox).unwrap());
		}
		pyramid
	}

	/// Intersects each bounding box in the pyramid with the bounding box derived from the provided [`GeoBBox`].
	///
	/// # Arguments
	///
	/// * `geo_bbox` - The geographical bounding box to intersect with.
	pub fn intersect_geo_bbox(&mut self, geo_bbox: &GeoBBox) {
		for (z, tile_bbox) in self.level_bbox.iter_mut().enumerate() {
			tile_bbox
				.intersect_bbox(&TileBBox::from_geo(z as u8, geo_bbox).unwrap())
				.unwrap();
		}
	}

	/// Expands each bounding box in the pyramid by the specified border offsets.
	///
	/// This effectively shifts each bounding box outward by `(x_min, y_min, x_max, y_max)`.
	/// If a bounding box is already empty, adding a border does nothing.
	pub fn add_border(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) {
		for bbox in self.level_bbox.iter_mut() {
			bbox.add_border(x_min, y_min, x_max, y_max);
		}
	}

	/// Intersects (in-place) this pyramid with another [`TileBBoxPyramid`].
	///
	/// Each zoom level is intersected independently with the corresponding level in `other_bbox_pyramid`.
	pub fn intersect(&mut self, other_bbox_pyramid: &TileBBoxPyramid) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			let other_bbox = other_bbox_pyramid.get_level_bbox(level as u8);
			bbox.intersect_bbox(other_bbox).unwrap();
		}
	}

	/// Returns a reference to the bounding box at the specified zoom level.
	///
	/// # Panics
	///
	/// Panics if `level` >= `MAX_ZOOM_LEVEL`.
	pub fn get_level_bbox(&self, level: u8) -> &TileBBox {
		&self.level_bbox[level as usize]
	}

	/// Sets (in-place) the bounding box at the specified zoom level.
	///
	/// # Panics
	///
	/// Panics if `level` >= `MAX_ZOOM_LEVEL`.
	pub fn set_level_bbox(&mut self, bbox: TileBBox) {
		let level = bbox.level as usize;
		self.level_bbox[level] = bbox;
	}

	/// Includes a single tile coordinate in the pyramid, updating the bounding box
	/// at the coordinate’s zoom level to ensure it now encompasses `(x, y)`.
	pub fn include_coord(&mut self, coord: &TileCoord3) {
		self.level_bbox[coord.z as usize].include_coord(coord.x, coord.y)
	}

	/// Includes another bounding box in the pyramid, merging it with the existing bounding box
	/// at that bounding box’s zoom level.
	pub fn include_bbox(&mut self, bbox: &TileBBox) {
		self.level_bbox[bbox.level as usize].include_bbox(bbox).unwrap();
	}

	/// Includes all bounding boxes from another `TileBBoxPyramid` into this pyramid.
	///
	/// Each zoom level from `pyramid` is included into the corresponding level in `self`.
	pub fn include_bbox_pyramid(&mut self, pyramid: &TileBBoxPyramid) {
		for bbox in pyramid.iter_levels() {
			self.level_bbox[bbox.level as usize].include_bbox(bbox).unwrap();
		}
	}

	/// Checks if the pyramid contains the given `(x, y, z)` tile coordinate.
	pub fn contains_coord(&self, coord: &TileCoord3) -> bool {
		if let Some(bbox) = self.level_bbox.get(coord.z as usize) {
			bbox.contains3(coord)
		} else {
			false
		}
	}

	/// Checks if the pyramid overlaps the specified bounding box at the bounding box’s zoom level.
	pub fn overlaps_bbox(&self, bbox: &TileBBox) -> bool {
		if let Some(local_bbox) = self.level_bbox.get(bbox.level as usize) {
			local_bbox.overlaps_bbox(bbox).unwrap_or(false)
		} else {
			false
		}
	}

	/// Returns an iterator over all **non-empty** bounding boxes in this pyramid.
	///
	/// # Examples
	///
	/// ```
	/// # use versatiles_core::types::TileBBoxPyramid;
	/// // Suppose `pyramid` is a filled pyramid...
	/// // for bbox in pyramid.iter_levels() {
	/// //     println!("Level {} has some tiles", bbox.level);
	/// // }
	/// ```
	pub fn iter_levels(&self) -> impl Iterator<Item = &TileBBox> {
		self.level_bbox.iter().filter(|bbox| !bbox.is_empty())
	}

	/// Finds the minimum zoom level that contains any tiles.
	///
	/// Returns `None` if **all** levels are empty.
	pub fn get_zoom_min(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.find(|bbox| !bbox.is_empty())
			.map(|bbox| bbox.level)
	}

	/// Finds the maximum zoom level that contains any tiles.
	///
	/// Returns `None` if **all** levels are empty.
	pub fn get_zoom_max(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.rev()
			.find(|bbox| !bbox.is_empty())
			.map(|bbox| bbox.level)
	}

	/// Returns a “good” zoom level, heuristically one that has more than 10 tiles.
	///
	/// This scans from the highest zoom level downward, returning the first that meets
	/// a threshold of `> 10` tiles. Returns `None` if none meet that threshold.
	pub fn get_good_zoom(&self) -> Option<u8> {
		self
			.level_bbox
			.iter()
			.rev()
			.find(|bbox| bbox.count_tiles() > 10)
			.map(|bbox| bbox.level)
	}

	/// Clears bounding boxes for all levels < `zoom_level_min`.
	pub fn set_zoom_min(&mut self, zoom_level_min: u8) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			if (index as u8) < zoom_level_min {
				bbox.set_empty();
			}
		}
	}

	/// Clears bounding boxes for all levels > `zoom_level_max`.
	pub fn set_zoom_max(&mut self, zoom_level_max: u8) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			if (index as u8) > zoom_level_max {
				bbox.set_empty();
			}
		}
	}

	/// Counts the total number of tiles across all non-empty bounding boxes in this pyramid.
	pub fn count_tiles(&self) -> u64 {
		self.level_bbox.iter().map(|bbox| bbox.count_tiles()).sum()
	}

	/// Checks if **all** bounding boxes in this pyramid are empty.
	pub fn is_empty(&self) -> bool {
		self.level_bbox.iter().all(|bbox| bbox.is_empty())
	}

	/// Checks if this pyramid is “full” up to the specified zoom level, meaning
	/// each relevant bounding box is flagged as full coverage.
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

	/// Determines a geographical bounding box from the highest zoom level that contains tiles.
	///
	/// Returns `None` if the pyramid is empty.
	pub fn get_geo_bbox(&self) -> Option<GeoBBox> {
		let max_zoom = self.get_zoom_max()?;
		Some(self.get_level_bbox(max_zoom).as_geo_bbox())
	}

	/// Calculates a geographic center based on the bounding box at a middle zoom level.
	///
	/// This tries to pick a zoom that is “2 levels above the min,” but not exceeding the max.
	/// Returns `None` if the pyramid is empty or if the bounding box is invalid.
	pub fn get_geo_center(&self) -> Option<GeoCenter> {
		let bbox = self.get_geo_bbox()?;
		let zoom = (self.get_zoom_min()? + 2).min(self.get_zoom_max()?);
		let center_lon = (bbox.0 + bbox.2) / 2.0;
		let center_lat = (bbox.1 + bbox.3) / 2.0;
		Some(GeoCenter(center_lon, center_lat, zoom))
	}
}

impl fmt::Debug for TileBBoxPyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Debug: show only non-empty levels
		f.debug_list().entries(self.iter_levels()).finish()
	}
}

impl fmt::Display for TileBBoxPyramid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Display: also show only non-empty levels
		f.debug_list().entries(self.iter_levels()).finish()
	}
}

impl PartialEq for TileBBoxPyramid {
	fn eq(&self, other: &Self) -> bool {
		for level in 0..MAX_ZOOM_LEVEL {
			let bbox0 = self.get_level_bbox(level);
			let bbox1 = other.get_level_bbox(level);
			// If one is empty and the other is not, they're not equal
			if bbox0.is_empty() != bbox1.is_empty() {
				return false;
			}
			// If both are empty, skip
			if bbox0.is_empty() {
				continue;
			}
			// Otherwise, compare
			if bbox0 != bbox1 {
				return false;
			}
		}
		true
	}
}

impl Default for TileBBoxPyramid {
	/// Creates a new `TileBBoxPyramid` with all levels empty.
	fn default() -> Self {
		Self::new_empty()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;

	#[test]
	fn test_empty_pyramid() {
		let pyramid = TileBBoxPyramid::new_empty();
		assert!(
			pyramid.is_empty(),
			"Expected new_empty to create an entirely empty pyramid."
		);
		assert_eq!(pyramid.get_zoom_min(), None);
		assert_eq!(pyramid.get_zoom_max(), None);
		assert_eq!(pyramid.count_tiles(), 0);
	}

	#[test]
	fn test_full_pyramid() {
		let pyramid = TileBBoxPyramid::new_full(8);
		assert!(!pyramid.is_empty(), "A 'full' pyramid at level 8 is not empty.");
		// For testing, we expect it to be 'full' up to level 8
		assert!(pyramid.is_full(8));
		// Levels above 8 are empty
		for lvl in 9..MAX_ZOOM_LEVEL {
			assert!(pyramid.get_level_bbox(lvl).is_empty());
		}
	}

	#[test]
	fn test_intersections() {
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
	fn test_limit_by_geo_bbox() {
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
	fn test_include_coord2() -> Result<()> {
		let mut pyramid = TileBBoxPyramid::new_empty();
		pyramid.include_coord(&TileCoord3::new(1, 2, 3)?);
		pyramid.include_coord(&TileCoord3::new(4, 5, 3)?);
		pyramid.include_coord(&TileCoord3::new(6, 7, 8)?);

		assert!(pyramid.get_level_bbox(0).is_empty());
		assert!(pyramid.get_level_bbox(1).is_empty());
		assert!(pyramid.get_level_bbox(2).is_empty());
		assert_eq!(pyramid.get_level_bbox(3), &TileBBox::new(3, 1, 2, 4, 5)?);
		assert!(pyramid.get_level_bbox(4).is_empty());
		assert!(pyramid.get_level_bbox(5).is_empty());
		assert!(pyramid.get_level_bbox(6).is_empty());
		assert!(pyramid.get_level_bbox(7).is_empty());
		assert_eq!(pyramid.get_level_bbox(8), &TileBBox::new(8, 6, 7, 6, 7)?);
		assert!(pyramid.get_level_bbox(9).is_empty());

		Ok(())
	}

	#[test]
	fn test_include_bbox2() {
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
	fn test_level_bbox() {
		let test = |level: u8| {
			let mut pyramid = TileBBoxPyramid::new_empty();
			let bbox = TileBBox::new_full(level).unwrap();
			pyramid.set_level_bbox(bbox);
			assert_eq!(pyramid.get_level_bbox(level), &bbox);
		};

		test(0);
		test(1);
		test(30);
		test(31);
	}

	#[test]
	fn test_zoom_min_max2() {
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
	fn test_add_border1() {
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

	#[test]
	fn test_from_geo_bbox() {
		let bbox = GeoBBox(-10.0, -5.0, 10.0, 5.0);
		let pyramid = TileBBoxPyramid::from_geo_bbox(1, 3, &bbox);
		assert!(pyramid.get_level_bbox(0).is_empty());
		assert!(!pyramid.get_level_bbox(1).is_empty());
		assert!(!pyramid.get_level_bbox(2).is_empty());
		assert!(!pyramid.get_level_bbox(3).is_empty());
		assert!(pyramid.get_level_bbox(4).is_empty());
	}

	#[test]
	fn test_intersect_geo_bbox() {
		let mut pyramid = TileBBoxPyramid::new_full(5);
		let geo_bbox = GeoBBox(-5.0, -2.0, 3.0, 4.0);
		pyramid.intersect_geo_bbox(&geo_bbox);
		// Now we have a partial coverage at each level up to 5
		assert!(!pyramid.is_empty());
		// We won't check exact tile coords since that depends on the TileBBox logic,
		// but we can check that level 6+ is still empty:
		assert!(pyramid.get_level_bbox(6).is_empty());
	}

	#[test]
	fn test_add_border2() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		// Adding a border to an empty pyramid does nothing
		pyramid.add_border(1, 2, 3, 4);
		assert!(pyramid.is_empty());

		// If we create a partial pyramid and then add a border,
		// each bounding box should expand. We'll rely on the internal tests
		// of `TileBBox` to verify correctness.
		let mut pyramid2 = TileBBoxPyramid::new_full(3);
		pyramid2.add_border(2, 2, 4, 4);
		// We can't easily test exact numeric outcomes without replicating tile logic,
		// but we can check that it's still not empty.
		assert!(!pyramid2.is_empty());
	}

	#[test]
	fn test_intersect() {
		let mut p1 = TileBBoxPyramid::new_full(3);
		let p2 = TileBBoxPyramid::new_empty();

		p1.intersect(&p2);
		assert!(
			p1.is_empty(),
			"Intersecting a full pyramid with an empty one yields empty."
		);

		let mut p3 = TileBBoxPyramid::new_full(3);
		let p4 = TileBBoxPyramid::new_full(3);
		p3.intersect(&p4);
		assert!(p3.is_full(3), "Full ∩ full = full at the same levels.");
	}

	#[test]
	fn test_get_level_bbox() {
		let pyramid = TileBBoxPyramid::new_full(2);
		// Level 0, 1, 2 are full, 3 is empty
		assert!(pyramid.get_level_bbox(3).is_empty());
	}

	#[test]
	fn test_set_level_bbox() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		let custom_bbox = TileBBox::new_full(3).unwrap();
		pyramid.set_level_bbox(custom_bbox);
		assert_eq!(pyramid.get_level_bbox(3), &custom_bbox);
	}

	#[test]
	fn test_include_coord1() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		let coord = TileCoord3::new(5, 10, 15).unwrap();
		pyramid.include_coord(&coord);
		assert!(!pyramid.get_level_bbox(15).is_empty());
	}

	#[test]
	fn test_include_bbox1() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		let tb = TileBBox::new(6, 10, 10, 12, 12).unwrap();
		pyramid.include_bbox(&tb);
		assert!(!pyramid.get_level_bbox(6).is_empty());
		// No other level should be affected
		assert!(pyramid.get_level_bbox(5).is_empty());
		assert!(pyramid.get_level_bbox(7).is_empty());
	}

	#[test]
	fn test_include_bbox1_pyramid() {
		let mut p1 = TileBBoxPyramid::new_empty();
		let p2 = TileBBoxPyramid::new_full(2);
		p1.include_bbox_pyramid(&p2);
		// Now p1 should have coverage at levels 0..=2
		assert!(p1.get_level_bbox(0).is_full());
		assert!(p1.get_level_bbox(1).is_full());
		assert!(p1.get_level_bbox(2).is_full());
		assert!(p1.get_level_bbox(3).is_empty());
	}

	#[test]
	fn test_contains_coord() {
		let mut p = TileBBoxPyramid::new_empty();
		p.include_bbox(&TileBBox::new(10, 100, 200, 300, 400).unwrap());
		assert!(!p.contains_coord(&TileCoord3::new(99, 200, 10).unwrap()));
		assert!(!p.contains_coord(&TileCoord3::new(100, 199, 10).unwrap()));
		assert!(p.contains_coord(&TileCoord3::new(100, 200, 10).unwrap()));
		assert!(p.contains_coord(&TileCoord3::new(300, 400, 10).unwrap()));
		assert!(!p.contains_coord(&TileCoord3::new(301, 400, 10).unwrap()));
		assert!(!p.contains_coord(&TileCoord3::new(300, 401, 10).unwrap()));
		assert!(!p.contains_coord(&TileCoord3::new(300, 400, 11).unwrap()));
	}

	#[test]
	fn test_overlaps_bbox() {
		let mut p = TileBBoxPyramid::new_empty();
		p.include_bbox(&TileBBox::new(10, 100, 200, 300, 400).unwrap());
		assert!(!p.overlaps_bbox(&TileBBox::new(10, 0, 0, 99, 200).unwrap()));
		assert!(!p.overlaps_bbox(&TileBBox::new(10, 0, 0, 100, 199).unwrap()));
		assert!(p.overlaps_bbox(&TileBBox::new(10, 0, 0, 100, 200).unwrap()));
		assert!(p.overlaps_bbox(&TileBBox::new(10, 300, 400, 500, 600).unwrap()));
		assert!(!p.overlaps_bbox(&TileBBox::new(10, 300, 401, 500, 600).unwrap()));
		assert!(!p.overlaps_bbox(&TileBBox::new(10, 301, 400, 500, 600).unwrap()));
		assert!(!p.overlaps_bbox(&TileBBox::new(11, 300, 400, 500, 600).unwrap()));
	}

	#[test]
	fn test_iter_levels() {
		let p = TileBBoxPyramid::new_full(2);
		let levels: Vec<u8> = p.iter_levels().map(|tb| tb.level).collect();
		assert_eq!(levels, vec![0, 1, 2]);
	}

	#[test]
	fn test_zoom_min_max1() {
		let p = TileBBoxPyramid::new_full(3);
		assert_eq!(p.get_zoom_min(), Some(0));
		assert_eq!(p.get_zoom_max(), Some(3));

		let empty_p = TileBBoxPyramid::new_empty();
		assert_eq!(empty_p.get_zoom_min(), None);
		assert_eq!(empty_p.get_zoom_max(), None);
	}

	#[test]
	fn test_get_good_zoom() {
		let p = TileBBoxPyramid::new_full(5);
		// Usually, full coverage at level 5 implies many tiles, so we'd find a "good" zoom near 5.
		let good_zoom = p.get_good_zoom().unwrap();
		// We can't say exactly which level (tile logic is in TileBBox), but typically it'd be 4 or 5
		assert!(good_zoom <= 5);
	}

	#[test]
	fn test_set_zoom_min_max() {
		let mut p = TileBBoxPyramid::new_full(5);
		// We remove coverage below level 2
		p.set_zoom_min(2);
		assert_eq!(p.get_zoom_min(), Some(2));
		assert_eq!(p.get_zoom_max(), Some(5));

		// Then remove coverage above level 4
		p.set_zoom_max(4);
		assert_eq!(p.get_zoom_min(), Some(2));
		assert_eq!(p.get_zoom_max(), Some(4));
	}

	#[test]
	fn test_count_tiles() {
		let empty_p = TileBBoxPyramid::new_empty();
		assert_eq!(empty_p.count_tiles(), 0);

		// Full coverage typically has many tiles, though exact counts are not trivial
		// without replicating tile coverage logic. We'll just ensure it's not zero.
		let p = TileBBoxPyramid::new_full(2);
		assert!(p.count_tiles() > 0);
	}

	#[test]
	fn test_get_geo_bbox_and_center() {
		let p = TileBBoxPyramid::new_full(2);
		// At a basic level, we expect a bounding box covering the globe
		let maybe_bbox = p.get_geo_bbox();
		assert!(maybe_bbox.is_some());
		// The center then should be around (0, 0, some zoom)
		let maybe_center = p.get_geo_center();
		assert!(maybe_center.is_some());
	}
}
