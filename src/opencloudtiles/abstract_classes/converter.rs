#![allow(unused_variables)]

use std::path::PathBuf;

use crate::opencloudtiles::compress::*;

use super::{reader::TileBBox, Tile, TileFormat, TileReader, TileReaderParameters};

pub trait TileConverter {
	fn new(
		filename: &PathBuf,
		config: Option<TileConverterConfig>,
	) -> Result<Box<dyn TileConverter>, &'static str>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: Box<dyn TileReader>) -> Result<(), &'static str> {
		panic!()
	}
}

pub struct TileConverterConfig {
	min_zoom: Option<u64>,
	max_zoom: Option<u64>,
	tile_format: Option<TileFormat>,
	level_bbox: Vec<TileBBox>,
	tile_converter: Option<fn(&Tile) -> Tile>,
	force_recompress: bool,
}

impl TileConverterConfig {
	pub fn new_empty() -> Self {
		return TileConverterConfig {
			min_zoom: None,
			max_zoom: None,
			tile_format: None,
			level_bbox: Vec::new(),
			tile_converter: None,
			force_recompress: false,
		};
	}
	pub fn finalize_with_parameters(&mut self, parameters: &TileReaderParameters) {
		let min_zoom = parameters.get_min_zoom();
		if self.min_zoom.is_none() {
			self.min_zoom = Some(min_zoom);
		} else {
			self.min_zoom = Some(self.min_zoom.unwrap().max(min_zoom));
		}

		let max_zoom = parameters.get_max_zoom();
		if self.max_zoom.is_none() {
			self.max_zoom = Some(max_zoom);
		} else {
			self.max_zoom = Some(self.max_zoom.unwrap().min(max_zoom));
		}

		self.tile_converter = Some(self.calc_tile_converter(&parameters.get_tile_format()));
	}
	pub fn get_tile_converter(&self) -> fn(&Tile) -> Tile {
		return self.tile_converter.unwrap();
	}
	fn calc_tile_converter(&mut self, src_tile_format: &TileFormat) -> fn(&Tile) -> Tile {
		if self.tile_format.is_none() {
			self.tile_format = Some(src_tile_format.clone());
			return tile_same;
		}

		let dst_tile_format = self.tile_format.as_ref().unwrap();

		if src_tile_format == dst_tile_format {
			return tile_same;
		}

		//if src_tile_format == TileFormat::PNG | TileFormat::JPG | TileFormat::WEBP {}

		return match (src_tile_format, dst_tile_format) {
			(TileFormat::PNG, TileFormat::PNG) => tile_same,
			(TileFormat::PNG, _) => panic!(),

			(TileFormat::JPG, TileFormat::JPG) => tile_same,
			(TileFormat::JPG, _) => panic!(),

			(TileFormat::WEBP, TileFormat::WEBP) => tile_same,
			(TileFormat::WEBP, _) => panic!(),

			(TileFormat::PBF, TileFormat::PBF) => tile_same,
			(TileFormat::PBF, TileFormat::PBFBrotli) => compress_brotli,
			(TileFormat::PBF, TileFormat::PBFGzip) => compress_gzip,
			(TileFormat::PBF, _) => panic!(),

			(TileFormat::PBFBrotli, TileFormat::PBF) => decompress_brotli,
			(TileFormat::PBFBrotli, TileFormat::PBFBrotli) => {
				if self.force_recompress {
					fn tile_unbrotli_brotli(tile: &Tile) -> Tile {
						compress_brotli(&decompress_brotli(&tile))
					}
					tile_unbrotli_brotli
				} else {
					tile_same
				}
			}
			(TileFormat::PBFBrotli, TileFormat::PBFGzip) => {
				fn tile_unbrotli_gzip(tile: &Tile) -> Tile {
					compress_gzip(&decompress_brotli(&tile))
				}
				tile_unbrotli_gzip
			}
			(TileFormat::PBFBrotli, _) => panic!(),

			(TileFormat::PBFGzip, TileFormat::PBF) => decompress_gzip,
			(TileFormat::PBFGzip, TileFormat::PBFBrotli) => {
				fn tile_ungzip_brotli(tile: &Tile) -> Tile {
					compress_brotli(&&decompress_gzip(&tile))
				}
				tile_ungzip_brotli
			}
			(TileFormat::PBFGzip, TileFormat::PBFGzip) => {
				if self.force_recompress {
					fn tile_ungzip_gzip(tile: &Tile) -> Tile {
						compress_gzip(&decompress_gzip(&tile))
					}
					tile_ungzip_gzip
				} else {
					tile_same
				}
			}
			(TileFormat::PBFGzip, _) => todo!(),
		};

		fn tile_same(tile: &Tile) -> Tile {
			return tile.clone();
		}
	}
	pub fn get_min_zoom(&self) -> u64 {
		return self.min_zoom.unwrap();
	}
	pub fn get_max_zoom(&self) -> u64 {
		return self.max_zoom.unwrap();
	}
	pub fn get_zoom_bbox(&self, zoom: u64) -> Option<&TileBBox> {
		return self.level_bbox.get(zoom as usize);
	}
	pub fn set_min_zoom(&mut self, zoom: &Option<u64>) {
		if zoom.is_some() {
			self.min_zoom = Some(zoom.unwrap());
		}
	}
	pub fn set_max_zoom(&mut self, zoom: &Option<u64>) {
		if zoom.is_some() {
			self.max_zoom = Some(zoom.unwrap());
		}
	}
	pub fn set_tile_format(&mut self, tile_format: &Option<TileFormat>) {
		if tile_format.is_some() {
			self.tile_format = Some(tile_format.as_ref().unwrap().clone());
		}
	}
	pub fn set_recompress(&mut self, force_recompress: &Option<bool>) {
		if force_recompress.is_some() {
			self.force_recompress = self.force_recompress
		}
	}
}
