use clap::ValueEnum;

#[derive(Clone)]
pub struct TileBBox {
	row_min: u64,
	row_max: u64,
	col_min: u64,
	col_max: u64,
}

impl TileBBox {
	pub fn new(row_min: u64, row_max: u64, col_min: u64, col_max: u64) -> Self {
		TileBBox {
			row_min,
			row_max,
			col_min,
			col_max,
		}
	}
	pub fn get_row_min(&self) -> u64 {
		return self.row_min;
	}
	pub fn get_row_max(&self) -> u64 {
		return self.row_max;
	}
	pub fn get_col_min(&self) -> u64 {
		return self.col_min;
	}
	pub fn get_col_max(&self) -> u64 {
		return self.col_max;
	}
	pub fn intersect(&mut self, bbox: &TileBBox) {
		self.row_min = self.row_min.max(bbox.row_min);
		self.row_max = self.row_max.min(bbox.row_max);
		self.col_min = self.col_min.max(bbox.col_min);
		self.col_max = self.col_max.min(bbox.col_max);
	}
}

#[derive(PartialEq, Clone, Debug, ValueEnum)]
pub enum TileFormat {
	PBF,
	PBFGzip,
	PBFBrotli,
	PNG,
	JPG,
	WEBP,
}

pub type Tile = Vec<u8>;
