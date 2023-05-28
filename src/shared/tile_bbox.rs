use super::{TileCoord2, TileCoord3};
use itertools::Itertools;
use std::{
	fmt,
	ops::{Div, Rem},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TileBBox {
	pub x_min: u64,
	pub y_min: u64,
	pub x_max: u64,
	pub y_max: u64,
}

impl TileBBox {
	pub fn new(x_min: u64, y_min: u64, x_max: u64, y_max: u64) -> TileBBox {
		TileBBox {
			x_min,
			y_min,
			x_max,
			y_max,
		}
	}
	pub fn new_full(level: u8) -> TileBBox {
		let max = 2u64.pow(level as u32) - 1;
		TileBBox::new(0, 0, max, max)
	}
	pub fn new_empty() -> TileBBox {
		TileBBox::new(1, 1, 0, 0)
	}
	pub fn from_geo(geo_bbox: &[f32; 4], z: u8) -> TileBBox {
		let x_min: f32 = geo_bbox[0].min(geo_bbox[2]);
		let x_max: f32 = geo_bbox[0].max(geo_bbox[2]);
		let y_min: f32 = geo_bbox[1].min(geo_bbox[3]);
		let y_max: f32 = geo_bbox[1].max(geo_bbox[3]);

		let p_min = TileCoord2::from_geo(x_min, y_max, z, false);
		let p_max = TileCoord2::from_geo(x_max, y_min, z, true);

		let max_i = 2u64.pow(z as u32) - 1;

		TileBBox::new(
			p_min.x.max(0),
			p_min.y.max(0),
			max_i.min(p_max.x.max(1) - 1),
			max_i.min(p_max.y.max(1) - 1),
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
	#[cfg(test)]
	pub fn set_full(&mut self, level: u64) {
		let max = 2u64.pow(level as u32) - 1;
		self.x_min = 0;
		self.y_min = 0;
		self.x_max = max;
		self.y_max = max;
	}
	#[cfg(test)]
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
	pub fn add_border(&mut self, x_min: &u64, y_min: &u64, x_max: &u64, y_max: &u64) {
		if self.is_empty() {
		} else {
			self.x_min -= x_min;
			self.y_min -= y_min;
			self.x_max += x_max;
			self.y_max += y_max;
		}
	}
	pub fn set_bbox(&mut self, bbox: &TileBBox) {
		self.x_min = bbox.x_min;
		self.y_min = bbox.y_min;
		self.x_max = bbox.x_max;
		self.y_max = bbox.y_max;
	}
	pub fn iter_coords(&self) -> impl Iterator<Item = TileCoord2> + '_ {
		let y_range = self.y_min..=self.y_max;
		let x_range = self.x_min..=self.x_max;
		y_range.cartesian_product(x_range).map(|(y, x)| TileCoord2::new(x, y))
	}
	pub fn iter_bbox_row_slices(&self, max_count: usize) -> impl Iterator<Item = TileBBox> + '_ {
		let mut col_count = (self.x_max - self.x_min + 1) as usize;
		let mut row_count = (self.y_max - self.y_min + 1) as usize;

		let mut col_pos: Vec<u64> = Vec::new();
		let mut row_pos: Vec<u64> = Vec::new();

		if max_count <= col_count {
			// split each row into chunks

			let col_chunk_count = (col_count as f64 / max_count as f64).ceil() as usize;
			let col_chunk_size = col_count as f64 / col_chunk_count as f64;
			for col in 0..=col_chunk_count {
				col_pos.insert(col, (col_chunk_size * col as f64) as u64 + self.x_min)
			}
			col_count = col_chunk_count;

			for row in self.y_min..=self.y_max + 1 {
				row_pos.insert((row - self.y_min) as usize, row)
			}
		} else {
			// each chunk consists of multiple rows

			let row_chunk_max_size = max_count / col_count;
			let row_chunk_count = (row_count as f64 / row_chunk_max_size as f64).ceil() as usize;
			let row_chunk_size = row_count as f64 / row_chunk_count as f64;
			for row in 0..=row_chunk_count {
				row_pos.insert(row, (row_chunk_size * row as f64).round() as u64 + self.y_min)
			}
			row_count = row_chunk_count;

			col_pos.insert(0, self.x_min);
			col_pos.insert(1, self.x_max + 1);
			col_count = 1;
		}

		assert_eq!(col_pos[0], self.x_min, "incorrect x_min");
		assert_eq!(row_pos[0], self.y_min, "incorrect y_min");
		assert_eq!(col_pos[col_count] - 1, self.x_max, "incorrect x_max");
		assert_eq!(row_pos[row_count] - 1, self.y_max, "incorrect y_max");

		let cols = 0..col_count;
		let rows = 0..row_count;

		rows
			.cartesian_product(cols)
			.map(move |(row, col)| TileBBox::new(col_pos[col], row_pos[row], col_pos[col + 1] - 1, row_pos[row + 1] - 1))
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
	pub fn contains(&self, coord: &TileCoord2) -> bool {
		(coord.x >= self.x_min) && (coord.x <= self.x_max) && (coord.y >= self.y_min) && (coord.y <= self.y_max)
	}
	pub fn get_tile_index(&self, coord: &TileCoord2) -> usize {
		if !self.contains(coord) {
			panic!("coord '{coord:?}' is not in '{self:?}'")
		}

		let x = coord.x - self.x_min;
		let y = coord.y - self.y_min;
		let index = y * (self.x_max + 1 - self.x_min) + x;

		index as usize
	}
	pub fn get_coord_by_index(&self, index: usize) -> TileCoord2 {
		let width = self.x_max + 1 - self.x_min;
		let i = index as u64;
		TileCoord2::new(i.rem(width) + self.x_min, i.div(width) + self.y_min)
	}
	pub fn as_geo_bbox(&self, z: u8) -> [f32; 4] {
		let p_min = TileCoord3::new(self.x_min, self.y_max + 1, z).as_geo();
		let p_max = TileCoord3::new(self.x_max + 1, self.y_min, z).as_geo();

		[p_min[0], p_min[1], p_max[0], p_max[1]]
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
		assert_eq!(TileBBox::new(5, 12, 5, 12).count_tiles(), 1);
		assert_eq!(TileBBox::new(5, 12, 7, 15).count_tiles(), 12);
		assert_eq!(TileBBox::new(5, 15, 7, 12).count_tiles(), 0);
		assert_eq!(TileBBox::new(7, 12, 5, 15).count_tiles(), 0);
	}

	#[test]
	fn from_geo() {
		let bbox1 = TileBBox::from_geo(&[8.0653f32, 51.3563f32, 12.3528f32, 52.2564f32], 9);
		let bbox2 = TileBBox::new(267, 168, 273, 170);
		println!("bbox1 {:?}", bbox1);
		println!("bbox2 {:?}", bbox2);
		assert_eq!(bbox1, bbox2);
	}

	#[test]
	fn quarter_planet() {
		let quarter_planet0 = [0.001f32, -90f32, 179.999f32, -0.001f32];
		let quarter_planet1 = [0f32, -85.05113f32, 180f32, 0f32];
		for level in 1..18 {
			let level_bbox0 = TileBBox::from_geo(&quarter_planet0, level);
			let geo_bbox = level_bbox0.as_geo_bbox(level);
			let level_bbox1 = TileBBox::from_geo(&geo_bbox, level);
			assert_eq!(geo_bbox, quarter_planet1);
			assert_eq!(level_bbox1, level_bbox0);
		}
	}

	#[test]
	fn sa_pacific() {
		let geo_bbox0 = [-180f32, -66.51326f32, -90f32, 0f32];
		for level in 2..32 {
			let level_bbox0 = TileBBox::from_geo(&geo_bbox0, level);
			assert_eq!(level_bbox0.count_tiles(), 4u64.pow(level as u32 - 2));
			let geo_bbox1 = level_bbox0.as_geo_bbox(level);
			assert_eq!(geo_bbox1, geo_bbox0);
		}
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

	#[test]
	fn iter_coords() {
		let bbox = TileBBox::new(1, 5, 2, 6);
		let vec: Vec<TileCoord2> = bbox.iter_coords().collect();
		assert_eq!(vec.len(), 4);
		assert_eq!(vec[0], TileCoord2::new(1, 5));
		assert_eq!(vec[1], TileCoord2::new(2, 5));
		assert_eq!(vec[2], TileCoord2::new(1, 6));
		assert_eq!(vec[3], TileCoord2::new(2, 6));
	}

	#[test]
	fn iter_bbox_slices_99() {
		let bbox = TileBBox::new(0, 1000, 99, 1003);

		let vec: Vec<TileBBox> = bbox.iter_bbox_row_slices(99).collect();
		println!("{:?}", bbox);
		assert_eq!(vec.len(), 8);
		assert_eq!(vec[0], TileBBox::new(0, 1000, 49, 1000));
		assert_eq!(vec[1], TileBBox::new(50, 1000, 99, 1000));
		assert_eq!(vec[2], TileBBox::new(0, 1001, 49, 1001));
		assert_eq!(vec[3], TileBBox::new(50, 1001, 99, 1001));
		assert_eq!(vec[4], TileBBox::new(0, 1002, 49, 1002));
		assert_eq!(vec[5], TileBBox::new(50, 1002, 99, 1002));
		assert_eq!(vec[6], TileBBox::new(0, 1003, 49, 1003));
		assert_eq!(vec[7], TileBBox::new(50, 1003, 99, 1003));
	}

	#[test]
	fn iter_bbox_row_slices_100() {
		let bbox = TileBBox::new(0, 1000, 99, 1003);

		let vec: Vec<TileBBox> = bbox.iter_bbox_row_slices(100).collect();
		println!("{:?}", bbox);
		assert_eq!(vec.len(), 4);
		assert_eq!(vec[0], TileBBox::new(0, 1000, 99, 1000));
		assert_eq!(vec[1], TileBBox::new(0, 1001, 99, 1001));
		assert_eq!(vec[2], TileBBox::new(0, 1002, 99, 1002));
		assert_eq!(vec[3], TileBBox::new(0, 1003, 99, 1003));
	}

	#[test]
	fn iter_bbox_row_slices_199() {
		let bbox = TileBBox::new(0, 1000, 99, 1003);

		let vec: Vec<TileBBox> = bbox.iter_bbox_row_slices(199).collect();
		println!("{:?}", bbox);
		assert_eq!(vec.len(), 4);
		assert_eq!(vec[0], TileBBox::new(0, 1000, 99, 1000));
		assert_eq!(vec[1], TileBBox::new(0, 1001, 99, 1001));
		assert_eq!(vec[2], TileBBox::new(0, 1002, 99, 1002));
		assert_eq!(vec[3], TileBBox::new(0, 1003, 99, 1003));
	}

	#[test]
	fn iter_bbox_row_slices_200() {
		let bbox = TileBBox::new(0, 1000, 99, 1003);

		let vec: Vec<TileBBox> = bbox.iter_bbox_row_slices(200).collect();
		println!("{:?}", bbox);
		assert_eq!(vec.len(), 2);
		assert_eq!(vec[0], TileBBox::new(0, 1000, 99, 1001));
		assert_eq!(vec[1], TileBBox::new(0, 1002, 99, 1003));
	}
	#[test]

	fn add_border() {
		let mut bbox = TileBBox::new(5, 10, 20, 30);

		// border of (1, 1, 1, 1) should increase the size of the bbox by 1 in all directions
		bbox.add_border(&1, &1, &1, &1);
		assert_eq!(bbox, TileBBox::new(4, 9, 21, 31));

		// border of (2, 3, 4, 5) should further increase the size of the bbox
		bbox.add_border(&2, &3, &4, &5);
		assert_eq!(bbox, TileBBox::new(2, 6, 25, 36));

		// border of (0, 0, 0, 0) should not change the size of the bbox
		bbox.add_border(&0, &0, &0, &0);
		assert_eq!(bbox, TileBBox::new(2, 6, 25, 36));

		// if bbox is empty, add_border should have no effect
		let mut empty_bbox = TileBBox::new_empty();
		empty_bbox.add_border(&1, &2, &3, &4);
		assert_eq!(empty_bbox, TileBBox::new_empty());
	}
}
