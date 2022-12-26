use crate::opencloudtiles::{
	compress::*,
	image::*,
	types::{TileBBoxPyramide, TileData, TileFormat, TileReaderParameters},
};
use std::ops::RangeInclusive;

pub struct TileConverterConfig {
	tile_format: Option<TileFormat>,
	bbox_pyramide: TileBBoxPyramide,
	tile_converter: Option<TileDataConverter>,
	force_recompress: bool,
	finalized: bool,
}

impl TileConverterConfig {
	pub fn new(
		tile_format: Option<TileFormat>, bbox_pyramide: TileBBoxPyramide, force_recompress: bool,
	) -> Self {
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
	fn calc_tile_converter(&mut self, src_tile_format: &TileFormat) -> TileDataConverter {
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
			// ##### PNG
			(TileFormat::PNG, TileFormat::PNG) => {
				if self.force_recompress {
					|tile: &TileData| -> TileData { compress_png(&decompress_png(tile)) }
				} else {
					tile_same
				}
			}
			(TileFormat::PNG, TileFormat::JPG) => {
				|tile: &TileData| -> TileData { compress_jpg(&decompress_png(tile)) }
			}
			(TileFormat::PNG, TileFormat::WEBP) => {
				|tile: &TileData| -> TileData { compress_webp_lossless(&decompress_png(tile)) }
			}
			(TileFormat::PNG, _) => todo!("convert PNG -> ?"),

			// ##### JPEG
			(TileFormat::JPG, TileFormat::JPG) => tile_same,
			(TileFormat::JPG, TileFormat::PNG) => {
				|tile: &TileData| -> TileData { compress_png(&decompress_jpg(tile)) }
			}
			(TileFormat::JPG, TileFormat::WEBP) => {
				|tile: &TileData| -> TileData { compress_webp(&decompress_jpg(tile)) }
			}
			(TileFormat::JPG, _) => todo!("convert JPG -> ?"),

			(TileFormat::WEBP, TileFormat::WEBP) => tile_same,
			(TileFormat::WEBP, TileFormat::PNG) => {
				|tile: &TileData| -> TileData { compress_png(&decompress_webp(tile)) }
			}
			(TileFormat::WEBP, TileFormat::JPG) => {
				|tile: &TileData| -> TileData { compress_jpg(&decompress_webp(tile)) }
			}
			(TileFormat::WEBP, _) => todo!("convert WEBP -> ?"),

			(TileFormat::PBF, TileFormat::PBF) => tile_same,
			(TileFormat::PBF, TileFormat::PBFBrotli) => compress_brotli,
			(TileFormat::PBF, TileFormat::PBFGzip) => compress_gzip,
			(TileFormat::PBF, _) => todo!("convert PBF -> images"),

			(TileFormat::PBFBrotli, TileFormat::PBF) => decompress_brotli,
			(TileFormat::PBFBrotli, TileFormat::PBFBrotli) => {
				if self.force_recompress {
					|tile: &TileData| -> TileData { compress_brotli(&decompress_brotli(tile)) }
				} else {
					tile_same
				}
			}
			(TileFormat::PBFBrotli, TileFormat::PBFGzip) => {
				|tile: &TileData| -> TileData { compress_gzip(&decompress_brotli(tile)) }
			}
			(TileFormat::PBFBrotli, _) => todo!("convert PBFBrotli -> images"),

			(TileFormat::PBFGzip, TileFormat::PBF) => decompress_gzip,
			(TileFormat::PBFGzip, TileFormat::PBFBrotli) => {
				|tile: &TileData| -> TileData { compress_brotli(&decompress_gzip(tile)) }
			}
			(TileFormat::PBFGzip, TileFormat::PBFGzip) => {
				if self.force_recompress {
					|tile: &TileData| -> TileData { compress_gzip(&decompress_gzip(tile)) }
				} else {
					tile_same
				}
			}
			(TileFormat::PBFGzip, _) => todo!("convert PBFGzip -> images"),
		};

		fn tile_same(tile: &TileData) -> TileData {
			return tile.clone();
		}
	}
	pub fn get_tile_converter(&self) -> TileDataConverter {
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

type TileDataConverter = fn(&TileData) -> TileData;
