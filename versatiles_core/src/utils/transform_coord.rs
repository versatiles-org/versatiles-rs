//! Utilities for coordinate transformations on tile coordinates and bounding boxes.
//!
//! Provides the `TransformCoord` trait and implementations to flip and swap
//! axes for `TileCoord3`, `TileBBox`, and `TileBBoxPyramid`.

use crate::{TileBBox, TileBBoxPyramid, TileCoord3};
use std::mem::swap;

/// Trait for in-place coordinate transformations on tiles and bounding boxes.
///
/// Implementors can flip the y-axis or swap x/y coordinates.
pub trait TransformCoord {
	/// Flip along the vertical (y) axis. For coordinates, inverts y based on zoom level;
	/// for bounding boxes and pyramids, flips y-min/y-max accordingly.
	fn flip_y(&mut self);
	/// Swap the x and y axes in-place.
	fn swap_xy(&mut self);
}

impl TransformCoord for TileCoord3 {
	fn flip_y(&mut self) {
		let max_index = 2u32.pow(self.level as u32) - 1;
		assert!(max_index >= self.y, "error for {self:?}");
		self.y = max_index - self.y;
	}
	fn swap_xy(&mut self) {
		swap(&mut self.x, &mut self.y);
	}
}

impl TransformCoord for TileBBox {
	fn flip_y(&mut self) {
		if !self.is_empty() {
			let max = (1u32 << self.level) - 1;
			assert!(max >= self.y_max);
			self.y_min = max - self.y_min;
			self.y_max = max - self.y_max;
			swap(&mut self.y_min, &mut self.y_max);
		}
	}
	fn swap_xy(&mut self) {
		if !self.is_empty() {
			swap(&mut self.x_min, &mut self.y_min);
			swap(&mut self.x_max, &mut self.y_max);
		}
	}
}

impl TransformCoord for TileBBoxPyramid {
	fn swap_xy(&mut self) {
		self.level_bbox.iter_mut().for_each(|b| {
			b.swap_xy();
		});
	}
	fn flip_y(&mut self) {
		self.level_bbox.iter_mut().for_each(|b| {
			b.flip_y();
		});
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tilecoord3_flip_y() {
		let mut c = TileCoord3::new(3, 1, 2).unwrap();
		c.flip_y();
		assert_eq!(c, TileCoord3::new(3, 1, 5).unwrap());
	}

	#[test]
	fn tilecoord3_swap_xy() {
		let mut coord = TileCoord3::new(5, 3, 4).unwrap();
		coord.swap_xy();
		assert_eq!(coord, TileCoord3::new(5, 4, 3).unwrap());
	}

	#[test]
	fn bbox_flip_y() {
		let test = |a, b, c, d, e| -> TileBBox {
			let mut t = TileBBox::new(a, b, c, d, e).unwrap();
			t.flip_y();
			t
		};

		assert_eq!(test(1, 0, 0, 1, 1), TileBBox::new(1, 0, 0, 1, 1).unwrap());
		assert_eq!(test(2, 0, 0, 1, 1), TileBBox::new(2, 0, 2, 1, 3).unwrap());
		assert_eq!(test(3, 0, 0, 1, 1), TileBBox::new(3, 0, 6, 1, 7).unwrap());
		assert_eq!(test(9, 10, 0, 10, 511), TileBBox::new(9, 10, 0, 10, 511).unwrap());
		assert_eq!(test(9, 0, 10, 511, 10), TileBBox::new(9, 0, 501, 511, 501).unwrap());
	}

	#[test]
	fn bbox_swap_xy_transform() {
		let mut bbox = TileBBox::new(4, 1, 2, 3, 4).unwrap();
		bbox.swap_xy();
		assert_eq!(bbox, TileBBox::new(4, 2, 1, 4, 3).unwrap());
	}

	#[test]
	fn pyramid_swap_xy_transform() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		pyramid.include_bbox(&TileBBox::new(4, 0, 1, 2, 3).unwrap());
		pyramid.swap_xy();
		assert_eq!(pyramid.get_level_bbox(4), &TileBBox::new(4, 1, 0, 3, 2).unwrap());
	}

	#[test]
	fn pyramid_flip_y_transform() {
		let mut pyramid = TileBBoxPyramid::new_empty();
		pyramid.include_bbox(&TileBBox::new(4, 0, 1, 2, 3).unwrap());
		pyramid.flip_y();
		assert_eq!(pyramid.get_level_bbox(4), &TileBBox::new(4, 0, 12, 2, 14).unwrap());
	}
}
