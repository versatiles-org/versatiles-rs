use crate::{GeoBBox, TileBBox, TileBBoxPyramid, TileCoord};
use anyhow::Result;
use versatiles_derive::context;

impl TileBBoxPyramid {
	/// Intersects each bounding box in the pyramid with the bounding box derived from the provided [`GeoBBox`].
	///
	/// # Arguments
	///
	/// * `geo_bbox` - The geographical bounding box to intersect with.
	#[context("Failed to intersect {self} with {geo_bbox:?}")]
	pub fn intersect_geo_bbox(&mut self, geo_bbox: &GeoBBox) -> Result<()> {
		for (z, tile_bbox) in self.level_bbox.iter_mut().enumerate() {
			let level = u8::try_from(z).expect("zoom level index exceeds u8::MAX");
			tile_bbox.intersect_with(&TileBBox::from_geo(level, geo_bbox)?)?;
		}
		Ok(())
	}

	/// Expands each bounding box in the pyramid by the specified border offsets.
	///
	/// This effectively shifts each bounding box outward by `(x_min, y_min, x_max, y_max)`.
	/// If a bounding box is already empty, adding a border does nothing.
	pub fn add_border(&mut self, x_min: u32, y_min: u32, x_max: u32, y_max: u32) {
		for bbox in &mut self.level_bbox {
			bbox.expand_by(x_min, y_min, x_max, y_max);
		}
	}

	/// Intersects (in-place) this pyramid with another [`TileBBoxPyramid`].
	///
	/// Each zoom level is intersected independently with the corresponding level in `other_bbox_pyramid`.
	pub fn intersect(&mut self, other_bbox_pyramid: &TileBBoxPyramid) {
		for (level, bbox) in self.level_bbox.iter_mut().enumerate() {
			let level_u8 = u8::try_from(level).expect("zoom level index exceeds u8::MAX");
			let other_bbox = other_bbox_pyramid.get_level_bbox(level_u8);
			bbox.intersect_with(other_bbox).unwrap();
		}
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
	/// at the coordinate's zoom level to ensure it now encompasses `(x, y)`.
	pub fn include_coord(&mut self, coord: &TileCoord) {
		self.level_bbox[coord.level as usize].include(coord.x, coord.y);
	}

	/// Includes another bounding box in the pyramid, merging it with the existing bounding box
	/// at that bounding box's zoom level.
	pub fn include_bbox(&mut self, bbox: &TileBBox) {
		self.level_bbox[bbox.level as usize].include_bbox(bbox).unwrap();
	}

	/// Includes all bounding boxes from another `TileBBoxPyramid` into this pyramid.
	///
	/// Each zoom level from `pyramid` is included into the corresponding level in `self`.
	pub fn include_pyramid(&mut self, pyramid: &TileBBoxPyramid) {
		for bbox in pyramid.iter_levels() {
			self.level_bbox[bbox.level as usize].include_bbox(bbox).unwrap();
		}
	}

	/// Clears bounding boxes for all levels < `zoom_level_min`.
	pub fn set_level_min(&mut self, zoom_level_min: u8) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			let level = u8::try_from(index).expect("zoom level index exceeds u8::MAX");
			if level < zoom_level_min {
				bbox.set_empty();
			}
		}
	}

	/// Clears bounding boxes for all levels > `zoom_level_max`.
	pub fn set_level_max(&mut self, zoom_level_max: u8) {
		for (index, bbox) in self.level_bbox.iter_mut().enumerate() {
			let level = u8::try_from(index).expect("zoom level index exceeds u8::MAX");
			if level > zoom_level_max {
				bbox.set_empty();
			}
		}
	}

	pub fn swap_xy(&mut self) {
		self.level_bbox.iter_mut().for_each(|b| {
			b.swap_xy();
		});
	}

	pub fn flip_y(&mut self) {
		self.level_bbox.iter_mut().for_each(|b| {
			b.flip_y();
		});
	}
}
