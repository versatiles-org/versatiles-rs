use super::TileCoord2;
use itertools::Itertools;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct TileBBox {
	x_min: u64,
	y_min: u64,
	x_max: u64,
	y_max: u64,
}

#[allow(dead_code)]
impl TileBBox {
	pub fn new(x_min: u64, y_min: u64, x_max: u64, y_max: u64) -> TileBBox {
		TileBBox {
			x_min,
			y_min,
			x_max,
			y_max,
		}
	}
	pub fn new_full(level: u64) -> TileBBox {
		let max = 2u64.pow(level as u32) - 1;
		TileBBox::new(0, 0, max, max)
	}
	pub fn new_empty() -> TileBBox {
		TileBBox::new(1, 1, 0, 0)
	}
	pub fn from_geo(level: u64, geo_bbox: &[f32; 4]) -> TileBBox {
		let p1 = TileCoord2::from_geo(level, geo_bbox[0], geo_bbox[1]);
		let p2 = TileCoord2::from_geo(level, geo_bbox[2], geo_bbox[3]);

		TileBBox::new(
			p1.x.min(p2.x),
			p1.y.min(p2.y),
			p1.x.max(p2.x),
			p1.y.max(p2.y),
		)
	}
	pub fn set_empty(&mut self) {
		self.x_min = 1;
		self.y_min = 1;
		self.x_max = 0;
		self.y_max = 0;
	}
	pub fn is_empty(&self) -> bool {
		(self.x_max < self.x_min) || (self.y_max < self.y_min)
	}
	pub fn set_full(&mut self, level: u64) {
		let max = 2u64.pow(level as u32) - 1;
		self.x_min = 0;
		self.y_min = 0;
		self.x_max = max;
		self.y_max = max;
	}
	pub fn is_full(&self, level: u64) -> bool {
		let max = 2u64.pow(level as u32) - 1;
		(self.x_min == 0) && (self.y_min == 0) && (self.x_max == max) && (self.y_max == max)
	}
	pub fn count_tiles(&self) -> u64 {
		if self.x_max < self.x_min {
			return 0;
		}
		if self.y_max < self.y_min {
			return 0;
		}

		(self.x_max - self.x_min + 1) * (self.y_max - self.y_min + 1)
	}
	pub fn include_tile(&mut self, x: u64, y: u64) {
		if self.is_empty() {
			self.x_min = x;
			self.y_min = y;
			self.x_max = x;
			self.y_max = y;
		} else {
			self.x_min = self.x_min.min(x);
			self.y_min = self.y_min.min(y);
			self.x_max = self.x_max.max(x);
			self.y_max = self.y_max.max(y);
		}
	}
	pub fn union_bbox(&mut self, bbox: &TileBBox) {
		if self.is_empty() {
			self.set_bbox(bbox);
		} else {
			self.x_min = self.x_min.min(bbox.x_min);
			self.y_min = self.y_min.min(bbox.y_min);
			self.x_max = self.x_max.max(bbox.x_max);
			self.y_max = self.y_max.max(bbox.y_max);
		}
	}
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) {
		if self.is_empty() {
		} else {
			self.x_min = self.x_min.max(bbox.x_min);
			self.y_min = self.y_min.max(bbox.y_min);
			self.x_max = self.x_max.min(bbox.x_max);
			self.y_max = self.y_max.min(bbox.y_max);
		}
	}
	pub fn set_bbox(&mut self, bbox: &TileBBox) {
		self.x_min = bbox.x_min;
		self.y_min = bbox.y_min;
		self.x_max = bbox.x_max;
		self.y_max = bbox.y_max;
	}
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord2> {
		let y_values = self.y_min..=self.y_max;
		let x_values = self.x_min..=self.x_max;

		y_values
			.cartesian_product(x_values)
			.map(|(y, x)| TileCoord2 { x, y })
	}
	pub fn shift_by(mut self, x: u64, y: u64) -> TileBBox {
		self.x_min += x;
		self.y_min += y;
		self.x_max += x;
		self.y_max += y;

		self
	}
	pub fn scale_down(mut self, scale: u64) -> TileBBox {
		self.x_min /= scale;
		self.y_min /= scale;
		self.x_max /= scale;
		self.y_max /= scale;

		self
	}
	/*
	pub fn clamped_offset_from(mut self, x: u64, y: u64) -> TileBBox {
		self.x_min = (self.x_min.max(x) - x).min(255);
		self.y_min = (self.y_min.max(y) - y).min(255);
		self.x_max = (self.x_max.max(x) - x).min(255);
		self.y_max = (self.y_max.max(y) - y).min(255);

		self
	}
	*/
	pub fn contains(&self, coord: &TileCoord2) -> bool {
		(coord.x >= self.x_min)
			&& (coord.x <= self.x_max)
			&& (coord.y >= self.y_min)
			&& (coord.y <= self.y_max)
	}
	pub fn get_tile_index(&self, coord: &TileCoord2) -> usize {
		if !self.contains(coord) {
			panic!()
		}

		let x = coord.x - self.x_min;
		let y = coord.y - self.y_min;
		let index = y * (self.x_max + 1 - self.x_min) + x;

		index as usize
	}
	pub fn get_x_min(&self) -> u64 {
		self.x_min
	}
	pub fn get_y_min(&self) -> u64 {
		self.y_min
	}
	pub fn get_x_max(&self) -> u64 {
		self.x_max
	}
	pub fn get_y_max(&self) -> u64 {
		self.y_max
	}
}

impl fmt::Debug for TileBBox {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!(
			"TileBBox [{},{},{},{}] = {}",
			&self.x_min,
			&self.y_min,
			&self.x_max,
			&self.y_max,
			&self.count_tiles()
		))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn count_tiles() {
		assert_eq!(TileBBox::new(5, 12, 7, 15).count_tiles(), 12);
		assert_eq!(TileBBox::new(7, 12, 5, 15).count_tiles(), 0);
	}

	#[test]
	fn from_geo() {
		let bbox1 = TileBBox::from_geo(9, &[8.0653f32, 51.3563f32, 12.3528f32, 52.2564f32]);
		let bbox2 = TileBBox::new(267, 168, 273, 170);
		println!("bbox1 {:?}", bbox1);
		println!("bbox2 {:?}", bbox2);
		assert_eq!(bbox1, bbox2);
	}

	#[test]
	fn get_tile_index() {
		let bbox = TileBBox::new(100, 100, 199, 199);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(100, 100)), 0);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(101, 100)), 1);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(199, 100)), 99);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(100, 101)), 100);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(100, 199)), 9900);
		assert_eq!(bbox.get_tile_index(&TileCoord2::new(199, 199)), 9999);
	}

	#[test]
	fn boolean() {
		/*
			 #---#
		  #---# |
		  | | | |
		  | #-|-#
		  #---#
		*/
		let bbox1 = TileBBox::new(0, 11, 2, 13);
		let bbox2 = TileBBox::new(1, 10, 3, 12);

		let mut bbox1_intersect = bbox1.clone();
		bbox1_intersect.intersect_bbox(&bbox2);
		assert_eq!(bbox1_intersect, TileBBox::new(1, 11, 2, 12));

		let mut bbox1_union = bbox1;
		bbox1_union.union_bbox(&bbox2);
		assert_eq!(bbox1_union, TileBBox::new(0, 10, 3, 13));
	}

	#[test]
	fn include_tile() {
		let mut bbox = TileBBox::new(0, 1, 2, 3);
		bbox.include_tile(4, 5);
		assert_eq!(bbox, TileBBox::new(0, 1, 4, 5));
	}

	#[test]
	fn empty_or_full() {
		let mut bbox1 = TileBBox::new_empty();
		assert!(bbox1.is_empty());

		bbox1.set_full(12);
		assert!(bbox1.is_full(12));

		let mut bbox1 = TileBBox::new_full(13);
		assert!(bbox1.is_full(13));

		bbox1.set_empty();
		assert!(bbox1.is_empty());
	}
}
