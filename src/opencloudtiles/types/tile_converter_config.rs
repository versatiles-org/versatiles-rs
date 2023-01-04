use crate::opencloudtiles::{
	helpers::*,
	types::{TileBBoxPyramide, TileData, TileFormat, TileReaderParameters},
};

use super::Compression;

pub struct TileConverterConfig {
	tile_format: Option<TileFormat>,
	tile_precompression: Option<Compression>,
	tile_recompressor: Option<Vec<TileDataConverter>>,
	data_compressor: Option<TileDataConverter>,
	bbox_pyramide: TileBBoxPyramide,
	force_recompress: bool,
	finalized: bool,
}

impl TileConverterConfig {
	pub fn new(
		tile_format: Option<TileFormat>, tile_precompression: Option<Compression>,
		bbox_pyramide: TileBBoxPyramide, force_recompress: bool,
	) -> Self {
		return TileConverterConfig {
			tile_format,
			tile_precompression,
			bbox_pyramide,
			tile_recompressor: None,
			data_compressor: None,
			force_recompress,
			finalized: false,
		};
	}
	pub fn finalize_with_parameters(&mut self, parameters: &TileReaderParameters) {
		self.bbox_pyramide.intersect(parameters.get_level_bbox());

		self
			.tile_format
			.get_or_insert(parameters.get_tile_format().clone());
		self
			.tile_precompression
			.get_or_insert(parameters.get_tile_precompression().clone());

		self.tile_recompressor = Some(self.calc_tile_recompressor(parameters));
		self.data_compressor = Some(self.calc_data_compressor());

		self.finalized = true;
	}
	fn calc_tile_recompressor(&self, parameters: &TileReaderParameters) -> Vec<TileDataConverter> {
		let src_form = parameters.get_tile_format();
		let src_comp = parameters.get_tile_precompression();
		let dst_form = self.tile_format.as_ref().unwrap();
		let dst_comp = self.tile_precompression.as_ref().unwrap();

		let format_converter: Option<fn(&TileData) -> TileData> =
			if (src_form != dst_form) || self.force_recompress {
				use TileFormat::*;
				Some(match (src_form, dst_form) {
					(PNG, JPG) => |tile| img2jpg(&png2img(tile)),
					(PNG, WEBP) => |tile| img2webplossless(&png2img(tile)),
					(PNG, _) => todo!("convert PNG -> {:?}", dst_form),

					(JPG, PNG) => |tile| img2png(&jpg2img(tile)),
					(JPG, WEBP) => |tile| img2webp(&jpg2img(tile)),
					(JPG, _) => todo!("convert JPG -> {:?}", dst_form),

					(WEBP, PNG) => |tile| img2png(&webp2img(tile)),
					(WEBP, JPG) => |tile| img2jpg(&webp2img(tile)),
					(WEBP, _) => todo!("convert WEBP -> {:?}", dst_form),

					(PBF, _) => todo!("convert PBF -> {:?}", dst_form),
				})
			} else {
				None
			};

		let mut result: Vec<TileDataConverter> = Vec::new();
		if (src_comp == dst_comp) && !self.force_recompress {
			if format_converter.is_some() {
				result.push(format_converter.unwrap())
			}
		} else {
			use Compression::*;
			match src_comp {
				Uncompressed => {}
				Gzip => result.push(decompress_gzip),
				Brotli => result.push(decompress_brotli),
			}
			if format_converter.is_some() {
				result.push(format_converter.unwrap())
			}
			match dst_comp {
				Uncompressed => {}
				Gzip => result.push(compress_gzip),
				Brotli => result.push(compress_brotli),
			}
		};

		return result;
	}
	fn calc_data_compressor(&self) -> TileDataConverter {
		use Compression::*;
		fn dont_change(tile: &TileData) -> TileData {
			return tile.clone();
		}

		return match self.tile_precompression.as_ref().unwrap() {
			Uncompressed => dont_change,
			Gzip => compress_gzip,
			Brotli => compress_brotli,
		};
	}
	pub fn get_tile_recompressor(&self) -> &Vec<TileDataConverter> {
		return self.tile_recompressor.as_ref().unwrap();
	}
	pub fn get_data_compressor(&self) -> TileDataConverter {
		return self.data_compressor.unwrap();
	}
	pub fn get_bbox_pyramide(&self) -> &TileBBoxPyramide {
		return &self.bbox_pyramide;
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		return self.tile_format.as_ref().unwrap();
	}
	pub fn get_tile_precompression(&self) -> &Compression {
		return self.tile_precompression.as_ref().unwrap();
	}
	pub fn get_max_zoom(&self) -> u64 {
		return self.bbox_pyramide.get_max_zoom();
	}
}

type TileDataConverter = fn(&TileData) -> TileData;
