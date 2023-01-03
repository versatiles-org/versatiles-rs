use super::TileCoord2;
use itertools::Itertools;
use std::{f32::consts::PI, fmt};

#[derive(Clone)]
pub struct TileBBox {
	x_min: u64,
	y_min: u64,
	x_max: u64,
	y_max: u64,
}

impl TileBBox {
	pub fn new(x_min: u64, y_min: u64, x_max: u64, y_max: u64) -> TileBBox {
		return TileBBox {
			x_min,
			y_min,
			x_max,
			y_max,
		};
	}
	pub fn new_full(level: u64) -> TileBBox {
		let max = 2u64.pow(level as u32) - 1;
		TileBBox::new(0, 0, max, max)
	}
	pub fn new_empty() -> TileBBox {
		TileBBox::new(1, 1, 0, 0)
	}
	pub fn from_geo(level: u64, geo_bbox: &[f32; 4]) -> TileBBox {
		let zoom: f32 = 2.0f32.powi(level as i32);
		let x_min = zoom * (geo_bbox[0] / 360.0 + 0.5);
		let y_min = zoom * (PI - ((geo_bbox[1] / 90.0 + 1.0) * PI / 4.0).tan().ln());
		let x_max = zoom * (geo_bbox[2] / 360.0 + 0.5);
		let y_max = zoom * (PI - ((geo_bbox[3] / 90.0 + 1.0) * PI / 4.0).tan().ln());
		return TileBBox::new(x_min as u64, y_min as u64, x_max as u64, y_max as u64);
	}
	pub fn is_empty(&self) -> bool {
		return (self.x_max < self.x_min) || (self.y_max < self.y_min);
	}
	pub fn count_tiles(&self) -> u64 {
		let cols_count;
		if self.x_max < self.x_min {
			return 0;
		} else {
			cols_count = self.x_max - self.x_min + 1
		};

		if self.y_max < self.y_min {
			return 0;
		} else {
			return cols_count * (self.y_max - self.y_min + 1);
		};
	}
	pub fn set_empty(&mut self) {
		self.x_min = 1;
		self.y_min = 1;
		self.x_max = 0;
		self.y_max = 0;
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
	pub fn include_bbox(&mut self, bbox: &TileBBox) {
		if self.is_empty() {
			self.set_bbox(bbox);
		} else {
			self.x_min = self.x_min.min(bbox.x_min);
			self.y_min = self.y_min.min(bbox.y_min);
			self.x_max = self.x_max.max(bbox.x_max);
			self.y_max = self.y_max.max(bbox.y_max);
		}
	}
	pub fn intersect(&mut self, bbox: &TileBBox) {
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
		return y_values
			.cartesian_product(x_values)
			.map(|(y, x)| TileCoord2 { x, y });
	}
	pub fn shift_by(mut self, x: u64, y: u64) -> TileBBox {
		self.x_min += x;
		self.y_min += y;
		self.x_max += x;
		self.y_max += y;
		return self;
	}
	pub fn scale_down(mut self, scale: u64) -> TileBBox {
		self.x_min /= scale;
		self.y_min /= scale;
		self.x_max /= scale;
		self.y_max /= scale;
		return self;
	}
	pub fn clamped_offset_from(mut self, x: u64, y: u64) -> TileBBox {
		self.x_min = (self.x_min.max(x) - x).min(255);
		self.y_min = (self.y_min.max(y) - y).min(255);
		self.x_max = (self.x_max.max(x) - x).min(255);
		self.y_max = (self.y_max.max(y) - y).min(255);
		return self;
	}
	pub fn contains(&self, coord: &TileCoord2) -> bool {
		return (coord.x >= self.x_min)
			&& (coord.x <= self.x_max)
			&& (coord.y >= self.y_min)
			&& (coord.y <= self.y_max);
	}
	pub fn get_tile_index(&self, coord: &TileCoord2) -> usize {
		if !self.contains(coord) {
			panic!()
		}

		let x = coord.x - self.x_min;
		let y = coord.y - self.y_min;
		let index = y * (self.x_max + 1 - self.x_min) + x;
		return index as usize;
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
