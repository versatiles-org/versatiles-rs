#![allow(unused_variables)]

use crate::opencloudtiles::{
	compress::*,
	types::{TileBBox, TileData, TileFormat},
	TileReader, TileReaderParameters,
};
use std::path::PathBuf;

pub trait TileConverter {
	fn new(filename: &PathBuf, config: TileConverterConfig) -> Box<dyn TileConverter>
	where
		Self: Sized,
	{
		panic!()
	}
	fn convert_from(&mut self, reader: Box<dyn TileReader>) {
		panic!()
	}
}

pub struct TileConverterConfig {
	zoom_min: Option<u64>,
	zoom_max: Option<u64>,
	geo_bbox: Option<[f32; 4]>,
	tile_format: Option<TileFormat>,
	level_bbox: Option<Vec<TileBBox>>,
	tile_converter: Option<fn(&TileData) -> TileData>,
	force_recompress: bool,
	finalized: bool,
}

impl TileConverterConfig {
	pub fn from_options(
		zoom_min: &Option<u64>,
		zoom_max: &Option<u64>,
		geo_bbox: &Option<Vec<f32>>,
		tile_format: &Option<TileFormat>,
		force_recompress: &bool,
	) -> Self {
		return TileConverterConfig {
			zoom_min: zoom_min.clone(),
			zoom_max: zoom_max.clone(),
			tile_format: tile_format.clone(),
			geo_bbox: geo_bbox.as_ref().map(|v| v.as_slice().try_into().unwrap()),
			level_bbox: None,
			tile_converter: None,
			force_recompress: *force_recompress,
			finalized: false,
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

		if self.level_bbox.is_some() {
			panic!("level_bbox should not be defined yet")
		}
		let src_level_bbox = parameters.get_level_bbox();
		let mut dst_level_bbox = Vec::new();
		for (index, bbox) in src_level_bbox.iter().enumerate() {
			let level = index as u64;
			if (level < zoom_min) || (level > zoom_max) {
				continue;
			}
			dst_level_bbox.insert(index, bbox.clone());
		}

		if self.geo_bbox.is_some() {
			for (level, bbox) in dst_level_bbox.iter_mut().enumerate() {
				bbox.intersect(&TileBBox::from_geo(level as u64, self.geo_bbox.unwrap()))
			}
		}

		self.level_bbox = Some(dst_level_bbox);

		self.tile_converter = Some(self.calc_tile_converter(&parameters.get_tile_format()));

		self.finalized = true;
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
	pub fn get_tile_converter(&self) -> fn(&TileData) -> TileData {
		if !self.finalized {
			panic!()
		}

		return self.tile_converter.unwrap();
	}
	pub fn get_zoom_min(&self) -> u64 {
		if !self.finalized {
			panic!()
		}

		return self.zoom_min.unwrap();
	}
	pub fn get_zoom_max(&self) -> u64 {
		if !self.finalized {
			panic!()
		}

		return self.zoom_max.unwrap();
	}
	pub fn get_zoom_bbox(&self, zoom: u64) -> Option<&TileBBox> {
		if !self.finalized {
			panic!()
		}

		return self.level_bbox.as_ref().unwrap().get(zoom as usize);
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		if !self.finalized {
			panic!()
		}

		return self.tile_format.as_ref().unwrap();
	}
}
