use std::ops::RangeInclusive;

use crate::opencloudtiles::{
	compress::*,
	types::{tile_bbox_pyramide::TileBBoxPyramide, tile_reader_parameters::TileReaderParameters, TileData, TileFormat},
};

pub struct TileConverterConfig {
	tile_format: Option<TileFormat>,
	bbox_pyramide: TileBBoxPyramide,
	tile_converter: Option<fn(&TileData) -> TileData>,
	force_recompress: bool,
	finalized: bool,
}

impl TileConverterConfig {
	pub fn new(tile_format: Option<TileFormat>, bbox_pyramide: TileBBoxPyramide, force_recompress: bool) -> Self {
		return TileConverterConfig {
			tile_format,
			bbox_pyramide,
			tile_converter: None,
			force_recompress,
			finalized: false,
		};
	}
	pub fn finalize_with_parameters(&mut self, parameters: &TileReaderParameters) {
		self.bbox_pyramide.intersect(parameters.get_level_bbox());

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
	pub fn get_bbox_pyramide(&self) -> &TileBBoxPyramide {
		return &self.bbox_pyramide;
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		if !self.finalized {
			panic!()
		}

		return self.tile_format.as_ref().unwrap();
	}
	pub fn get_zoom_range(&self) -> RangeInclusive<u64> {
		return self.bbox_pyramide.get_zoom_range();
	}
}
