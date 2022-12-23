#![allow(unused_variables)]

use crate::opencloudtiles::{
	compress::*,
	types::{TileBBox, TileData, TileFormat},
	TileReader, TileReaderParameters,
};
use std::path::PathBuf;

pub trait TileConverter {
	fn new(
		filename: &PathBuf,
		config: TileConverterConfig,
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
	zoom_min: Option<u64>,
	zoom_max: Option<u64>,
	tile_format: Option<TileFormat>,
	level_bbox: Vec<TileBBox>,
	tile_converter: Option<fn(&TileData) -> TileData>,
	force_recompress: bool,
}

impl TileConverterConfig {
	pub fn from_options(
		zoom_min: &Option<u64>,
		zoom_max: &Option<u64>,
		tile_format: &Option<TileFormat>,
		force_recompress: &Option<bool>,
	) -> Self {
		return TileConverterConfig {
			zoom_min: zoom_min.clone(),
			zoom_max: zoom_max.clone(),
			tile_format: tile_format.clone(),
			level_bbox: Vec::new(),
			tile_converter: None,
			force_recompress: force_recompress.unwrap_or(false),
		};
	}
	pub fn finalize_with_parameters(&mut self, parameters: &TileReaderParameters) {
		let zoom_min = parameters.get_zoom_min();
		if self.zoom_min.is_none() {
			self.zoom_min = Some(zoom_min);
		} else {
			self.zoom_min = Some(self.zoom_min.unwrap().max(zoom_min));
		}

		let zoom_max = parameters.get_zoom_max();
		if self.zoom_max.is_none() {
			self.zoom_max = Some(zoom_max);
		} else {
			self.zoom_max = Some(self.zoom_max.unwrap().min(zoom_max));
		}

		let level_bbox = parameters.get_level_bbox();

		for (level, bbox) in level_bbox.iter().enumerate() {
			let bbox_option = self.level_bbox.get_mut(level);
			if bbox_option.is_none() {
				self.level_bbox.insert(level, bbox.clone())
			} else {
				bbox_option.unwrap().intersect(bbox);
			}
		}

		// remove levels outside of [zoom_min, zoom_max]
		for index in 0..self.level_bbox.len() {
			let level = index as u64;
			if (level < zoom_min) || (level > zoom_max) {
				self.level_bbox.remove(index);
			}
		}

		self.tile_converter = Some(self.calc_tile_converter(&parameters.get_tile_format()));
	}
	pub fn get_tile_converter(&self) -> fn(&TileData) -> TileData {
		return self.tile_converter.unwrap();
	}
	fn calc_tile_converter(&mut self, src_tile_format: &TileFormat) -> fn(&TileData) -> TileData {
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
					fn tile_unbrotli_brotli(tile: &TileData) -> TileData {
						compress_brotli(&decompress_brotli(&tile))
					}
					tile_unbrotli_brotli
				} else {
					tile_same
				}
			}
			(TileFormat::PBFBrotli, TileFormat::PBFGzip) => {
				fn tile_unbrotli_gzip(tile: &TileData) -> TileData {
					compress_gzip(&decompress_brotli(&tile))
				}
				tile_unbrotli_gzip
			}
			(TileFormat::PBFBrotli, _) => panic!(),

			(TileFormat::PBFGzip, TileFormat::PBF) => decompress_gzip,
			(TileFormat::PBFGzip, TileFormat::PBFBrotli) => {
				fn tile_ungzip_brotli(tile: &TileData) -> TileData {
					compress_brotli(&&decompress_gzip(&tile))
				}
				tile_ungzip_brotli
			}
			(TileFormat::PBFGzip, TileFormat::PBFGzip) => {
				if self.force_recompress {
					fn tile_ungzip_gzip(tile: &TileData) -> TileData {
						compress_gzip(&decompress_gzip(&tile))
					}
					tile_ungzip_gzip
				} else {
					tile_same
				}
			}
			(TileFormat::PBFGzip, _) => todo!(),
		};

		fn tile_same(tile: &TileData) -> TileData {
			return tile.clone();
		}
	}
	pub fn get_zoom_min(&self) -> u64 {
		return self.zoom_min.unwrap();
	}
	pub fn get_zoom_max(&self) -> u64 {
		return self.zoom_max.unwrap();
	}
	pub fn get_zoom_bbox(&self, zoom: u64) -> Option<&TileBBox> {
		return self.level_bbox.get(zoom as usize);
	}
}
