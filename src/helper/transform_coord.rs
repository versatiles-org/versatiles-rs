use crate::types::{TileBBox, TileBBoxPyramid, TileCoord3};
use std::mem::swap;

pub trait TransformCoord {
	fn flip_y(&mut self);
	fn swap_xy(&mut self);
}

impl TransformCoord for TileCoord3 {
	fn flip_y(&mut self) {
		let max_index = 2u32.pow(self.z as u32) - 1;
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
			assert!(self.max >= self.y_max);
			self.y_min = self.max - self.y_min;
			self.y_max = self.max - self.y_max;
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
		let mut c = TileCoord3::new(1, 2, 3).unwrap();
		c.flip_y();
		assert_eq!(c, TileCoord3::new(1, 5, 3).unwrap());
	}

	#[test]
	fn tilecoord3_swap_xy() {
		let mut coord = TileCoord3::new(3, 4, 5).unwrap();
		coord.swap_xy();
		assert_eq!(coord, TileCoord3::new(4, 3, 5).unwrap());
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
}
