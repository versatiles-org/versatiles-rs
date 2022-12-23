#![allow(unused_variables)]

use std::path::PathBuf;

use super::{Tile, TileFormat};

pub trait TileReader {
	fn load(filename: &PathBuf) -> Result<Box<dyn TileReader>, &str>
	where
		Self: Sized,
	{
		panic!();
	}
	fn get_meta(&self) -> &[u8] {
		panic!();
	}
	fn get_parameters(&self) -> &TileReaderParameters {
		panic!();
	}
	fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Tile> {
		panic!();
	}
}

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

pub struct TileReaderParameters {
	zoom_min: u64,
	zoom_max: u64,
	level_bbox: Vec<TileBBox>,
	tile_format: TileFormat,
}

impl TileReaderParameters {
	pub fn new(
		zoom_min: u64,
		zoom_max: u64,
		tile_format: TileFormat,
		level_bbox: Vec<TileBBox>,
	) -> TileReaderParameters {
		return TileReaderParameters {
			zoom_min,
			zoom_max,
			tile_format,
			level_bbox,
		};
	}
	pub fn get_zoom_min(&self) -> u64 {
		return self.zoom_min;
	}
	pub fn get_zoom_max(&self) -> u64 {
		return self.zoom_max;
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		return &self.tile_format;
	}
	pub fn get_level_bbox(&self) -> &Vec<TileBBox> {
		return &self.level_bbox;
	}
}

pub struct TileReaderWrapper<'a> {
	reader: &'a Box<dyn TileReader>,
}

impl TileReaderWrapper<'_> {
	pub fn new(reader: &Box<dyn TileReader>) -> TileReaderWrapper {
		return TileReaderWrapper { reader };
	}
	pub fn get_tile_raw(&self, level: u64, col: u64, row: u64) -> Option<Tile> {
		return self.reader.get_tile_raw(level, col, row);
	}
}

unsafe impl Send for TileReaderWrapper<'_> {}
unsafe impl Sync for TileReaderWrapper<'_> {}
