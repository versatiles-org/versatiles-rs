#![allow(unused_variables)]

use crate::opencloudtiles::types::{TileBBox, TileData, TileFormat};
use std::path::PathBuf;

pub trait TileReader {
	fn load(filename: &PathBuf) -> Box<dyn TileReader>
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
	fn get_tile_data(&self, level: u64, col: u64, row: u64) -> Option<TileData> {
		panic!();
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
	pub fn get_tile_data(&self, level: u64, col: u64, row: u64) -> Option<TileData> {
		return self.reader.get_tile_data(level, col, row);
	}
}

unsafe impl Send for TileReaderWrapper<'_> {}
unsafe impl Sync for TileReaderWrapper<'_> {}
