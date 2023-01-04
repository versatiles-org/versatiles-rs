use crate::opencloudtiles::{
	helpers::*,
	types::{Blob, Precompression, TileFormat},
};

type FnConv = fn(Blob) -> Blob;

#[derive(Debug)]
pub struct DataConverter {
	pipeline: Vec<FnConv>,
}
impl DataConverter {
	fn empty() -> DataConverter {
		DataConverter {
			pipeline: Vec::new(),
		}
	}
	pub fn new_tile_recompressor(
		src_form: &TileFormat, src_comp: &Precompression, dst_form: &TileFormat,
		dst_comp: &Precompression, force_recompress: bool,
	) -> DataConverter {
		let mut converter = DataConverter::empty();

		let format_converter: Option<fn(Blob) -> Blob> = if (src_form != dst_form) || force_recompress
		{
			use TileFormat::*;
			match (src_form, dst_form) {
				(PNG, JPG) => Some(|tile| img2jpg(&png2img(tile))),
				(PNG, PNG) => Some(|tile| img2png(&png2img(tile))),
				(PNG, WEBP) => Some(|tile| img2webplossless(&png2img(tile))),
				(PNG, _) => todo!("convert PNG -> {:?}", dst_form),

				(JPG, JPG) => None,
				(JPG, PNG) => Some(|tile| img2png(&jpg2img(tile))),
				(JPG, WEBP) => Some(|tile| img2webp(&jpg2img(tile))),
				(JPG, _) => todo!("convert JPG -> {:?}", dst_form),

				(WEBP, JPG) => Some(|tile| img2jpg(&webp2img(tile))),
				(WEBP, PNG) => Some(|tile| img2png(&webp2img(tile))),
				(WEBP, WEBP) => None,
				(WEBP, _) => todo!("convert WEBP -> {:?}", dst_form),

				(PBF, PBF) => None,
				(PBF, _) => todo!("convert PBF -> {:?}", dst_form),
			}
		} else {
			None
		};

		if (src_comp == dst_comp) && !force_recompress {
			if format_converter.is_some() {
				converter.push(format_converter.unwrap())
			}
		} else {
			use Precompression::*;
			match src_comp {
				Uncompressed => {}
				Gzip => converter.push(decompress_gzip),
				Brotli => converter.push(decompress_brotli),
			}
			if format_converter.is_some() {
				converter.push(format_converter.unwrap())
			}
			match dst_comp {
				Uncompressed => {}
				Gzip => converter.push(compress_gzip),
				Brotli => converter.push(compress_brotli),
			}
		};

		return converter;
	}
	pub fn new_compressor(dst_comp: &Precompression) -> DataConverter {
		let mut converter = DataConverter::empty();

		match dst_comp {
			Precompression::Uncompressed => {}
			Precompression::Gzip => converter.push(compress_gzip),
			Precompression::Brotli => converter.push(compress_brotli),
		}

		return converter;
	}
	pub fn new_decompressor(src_comp: &Precompression) -> DataConverter {
		let mut converter = DataConverter::empty();

		match src_comp {
			Precompression::Uncompressed => {}
			Precompression::Gzip => converter.push(decompress_gzip),
			Precompression::Brotli => converter.push(decompress_brotli),
		}

		return converter;
	}
	fn push(&mut self, f: FnConv) {
		self.pipeline.push(f);
	}
	pub fn run(&self, mut data: Blob) -> Blob {
		for f in self.pipeline.iter() {
			data = f(data);
		}
		return data;
	}
}
