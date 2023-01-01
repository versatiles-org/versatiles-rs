use std::{f32::consts::PI, fmt};

use super::tile_coords::TileCoord2;

#[derive(Clone)]
pub struct TileBBox {
	pub x_min: u64,
	pub y_min: u64,
	pub x_max: u64,
	pub y_max: u64,
}

impl TileBBox {
	pub fn new(x_min: u64, y_min: u64, x_max: u64, y_max: u64) -> Self {
		TileBBox {
			x_min,
			y_min,
			x_max,
			y_max,
		}
	}
	pub fn new_full(level: u64) -> Self {
		let max = 2u64.pow(level as u32) - 1;
		TileBBox::new(0, 0, max, max)
	}
	pub fn new_empty(level: u64) -> Self {
		let max = 2u64.pow(level as u32);
		TileBBox::new(max, max, 0, 0)
	}
	pub fn from_geo(level: u64, geo_bbox: &[f32; 4]) -> Self {
		let zoom: f32 = 2.0f32.powi(level as i32);
		let x_min = zoom * (geo_bbox[0] / 360.0 + 0.5);
		let y_min = zoom * (PI - ((geo_bbox[1] / 90.0 + 1.0) * PI / 4.0).tan().ln());
		let x_max = zoom * (geo_bbox[2] / 360.0 + 0.5);
		let y_max = zoom * (PI - ((geo_bbox[3] / 90.0 + 1.0) * PI / 4.0).tan().ln());
		return TileBBox::new(x_min as u64, y_min as u64, x_max as u64, y_max as u64);
	}
	pub fn count_tiles(&self) -> u64 {
		let cols_count: u64 = if self.x_max < self.x_min {
			0
		} else {
			self.x_max - self.x_min + 1
		};
		let rows_count: u64 = if self.y_max < self.y_min {
			0
		} else {
			self.y_max - self.y_min + 1
		};
		return cols_count * rows_count;
	}
	pub fn set_empty(&mut self, level: u64) {
		let max = 2u64.pow(level as u32);
		self.x_min = max;
		self.y_min = max;
		self.x_max = 0;
		self.y_max = 0;
	}
	pub fn include_tile(&mut self, x: u64, y: u64) {
		if self.x_min > x {
			self.x_min = x
		}
		if self.y_min > y {
			self.y_min = y
		}
		if self.x_max < x {
			self.x_max = x
		}
		if self.y_max < y {
			self.y_max = y
		}
	}
	pub fn include_bbox(&mut self, bbox: &TileBBox) {
		if self.x_min > bbox.x_min {
			self.x_min = bbox.x_min
		}
		if self.y_min > bbox.y_min {
			self.y_min = bbox.y_min
		}
		if self.x_max < bbox.x_max {
			self.x_max = bbox.x_max
		}
		if self.y_max < bbox.y_max {
			self.y_max = bbox.y_max
		}
	}
	pub fn intersect(&mut self, bbox: &TileBBox) {
		self.x_min = self.x_min.max(bbox.x_min);
		self.y_min = self.y_min.max(bbox.y_min);
		self.x_max = self.x_max.min(bbox.x_max);
		self.y_max = self.y_max.min(bbox.y_max);
	}
	pub fn set(&mut self, bbox: &TileBBox) {
		self.x_min = bbox.x_min;
		self.y_min = bbox.y_min;
		self.x_max = bbox.x_max;
		self.y_max = bbox.y_max;
	}
	pub fn is_empty(&self) -> bool {
		return (self.x_max < self.x_min) || (self.y_max < self.y_min);
	}
	pub fn iter_tile_indexes(&self) -> impl Iterator<Item = TileCoord2> {
		let y_values = self.y_min..=self.y_max;
		let x_values = self.x_min..=self.x_max;
		return y_values
			.into_iter()
			.map(move |y| x_values.clone().into_iter().map(move |x| TileCoord2 { x, y }))
			.flatten();
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
}

impl fmt::Debug for TileBBox {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_fmt(format_args!(
			"TileBBox[{},{},{},{}]",
			&self.x_min, &self.y_min, &self.x_max, &self.y_max
		))
	}
}
