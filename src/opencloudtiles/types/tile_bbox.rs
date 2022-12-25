use std::f32::consts::PI;

#[derive(Clone)]
pub struct TileBBox {
	col_min: u64,
	row_min: u64,
	col_max: u64,
	row_max: u64,
}

impl TileBBox {
	pub fn new(col_min: u64, row_min: u64, col_max: u64, row_max: u64) -> Self {
		TileBBox {
			col_min,
			row_min,
			col_max,
			row_max,
		}
	}
	pub fn new_full(level: u64) -> Self {
		let max = 2u64.pow(level as u32);
		TileBBox::new(0, 0, max, max)
	}
	pub fn new_empty() -> Self {
		TileBBox::new(1, 1, 0, 0)
	}
	pub fn set_empty(&mut self) {
		self.col_min = 1;
		self.row_min = 1;
		self.col_max = 0;
		self.row_max = 0;
	}
	pub fn from_geo(level: u64, geo_bbox: [f32; 4]) -> Self {
		let zoom: f32 = 2.0f32.powi(level as i32);
		let x_min = zoom * (geo_bbox[0] / 360.0 + 0.5);
		let y_min = zoom * (PI - ((geo_bbox[1] / 90.0 + 1.0) * PI / 4.0).tan().ln());
		let x_max = zoom * (geo_bbox[2] / 360.0 + 0.5);
		let y_max = zoom * (PI - ((geo_bbox[3] / 90.0 + 1.0) * PI / 4.0).tan().ln());
		return TileBBox::new(x_min as u64, y_min as u64, x_max as u64, y_max as u64);
	}
	pub fn include_tile(&mut self, col: u64, row: u64) {
		if self.col_min > col {
			self.col_min = col
		}
		if self.row_min > row {
			self.row_min = row
		}
		if self.col_max < col {
			self.col_max = col
		}
		if self.row_max < row {
			self.row_max = row
		}
	}
	pub fn intersect(&mut self, bbox: &TileBBox) {
		self.col_min = self.col_min.max(bbox.col_min);
		self.row_min = self.row_min.max(bbox.row_min);
		self.col_max = self.col_max.min(bbox.col_max);
		self.row_max = self.row_max.min(bbox.row_max);
	}
	pub fn is_empty(&self) -> bool {
		return (self.col_max < self.col_min) || (self.row_max < self.row_min);
	}
	pub fn as_tuple(&self) -> (u64, u64, u64, u64) {
		return (self.col_min, self.row_min, self.col_max, self.row_max);
	}
}
