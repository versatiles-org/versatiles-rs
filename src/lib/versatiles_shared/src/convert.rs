use super::{compress::*, image::*, Blob, Precompression};
use clap::ValueEnum;
use std::fmt::Debug;

struct FnConv {
	func: fn(Blob) -> Blob,
	name: String,
}
impl FnConv {
	fn new(func: fn(Blob) -> Blob, name: &str) -> FnConv {
		FnConv {
			func,
			name: name.to_owned(),
		}
	}
	fn some(func: fn(Blob) -> Blob, name: &str) -> Option<FnConv> {
		Some(FnConv {
			func,
			name: name.to_owned(),
		})
	}
}

impl Debug for FnConv {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FnConv")
			.field("func", &self.func)
			.field("name", &self.name)
			.finish()
	}
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum TileFormat {
	BIN,
	PNG,
	JPG,
	WEBP,
	AVIF,
	SVG,
	PBF,
	GEOJSON,
	TOPOJSON,
	JSON,
}

#[derive(Debug)]
pub struct DataConverter {
	pipeline: Vec<FnConv>,
}
impl DataConverter {
	pub fn new_empty() -> DataConverter {
		DataConverter { pipeline: Vec::new() }
	}
	pub fn is_empty(&self) -> bool {
		self.pipeline.len() == 0
	}
	pub fn new_tile_recompressor(
		src_form: &TileFormat, src_comp: &Precompression, dst_form: &TileFormat, dst_comp: &Precompression,
		force_recompress: bool,
	) -> DataConverter {
		let mut converter = DataConverter::new_empty();

		let format_converter_option: Option<FnConv> = if (src_form != dst_form) || force_recompress {
			use TileFormat::*;
			match (src_form, dst_form) {
				(PNG, JPG) => FnConv::some(|tile| img2jpg(&png2img(tile)), "PNG->JPG"),
				(PNG, PNG) => FnConv::some(|tile| img2png(&png2img(tile)), "PNG->PNG"),
				(PNG, WEBP) => FnConv::some(|tile| img2webplossless(&png2img(tile)), "PNG->WEBP"),

				(JPG, PNG) => FnConv::some(|tile| img2png(&jpg2img(tile)), "JPG->PNG"),
				(JPG, WEBP) => FnConv::some(|tile| img2webp(&jpg2img(tile)), "JPG->WEBP"),

				(WEBP, JPG) => FnConv::some(|tile| img2jpg(&webp2img(tile)), "WEBP->JPG"),
				(WEBP, PNG) => FnConv::some(|tile| img2png(&webp2img(tile)), "WEBP->PNG"),

				(_, _) => {
					if src_form == dst_form {
						None
					} else {
						todo!("convert {:?} -> {:?}", src_form, dst_form)
					}
				}
			}
		} else {
			None
		};

		if (src_comp == dst_comp) && !force_recompress {
			if let Some(format_converter) = format_converter_option {
				converter.push(format_converter)
			}
		} else {
			use Precompression::*;
			match src_comp {
				Uncompressed => {}
				Gzip => converter.push(FnConv::new(decompress_gzip, "decompress_gzip")),
				Brotli => converter.push(FnConv::new(decompress_brotli, "decompress_brotli")),
			}
			if let Some(format_converter) = format_converter_option {
				converter.push(format_converter)
			}
			match dst_comp {
				Uncompressed => {}
				Gzip => converter.push(FnConv::new(compress_gzip, "compress_gzip")),
				Brotli => converter.push(FnConv::new(compress_brotli, "compress_brotli")),
			}
		};

		converter
	}
	pub fn new_compressor(dst_comp: &Precompression) -> DataConverter {
		let mut converter = DataConverter::new_empty();

		match dst_comp {
			Precompression::Uncompressed => {}
			Precompression::Gzip => converter.push(FnConv::new(compress_gzip, "compress_gzip")),
			Precompression::Brotli => converter.push(FnConv::new(compress_brotli, "compress_brotli")),
		}

		converter
	}
	pub fn new_decompressor(src_comp: &Precompression) -> DataConverter {
		let mut converter = DataConverter::new_empty();

		match src_comp {
			Precompression::Uncompressed => {}
			Precompression::Gzip => converter.push(FnConv::new(decompress_gzip, "decompress_gzip")),
			Precompression::Brotli => converter.push(FnConv::new(decompress_brotli, "decompress_brotli")),
		}

		converter
	}
	fn push(&mut self, f: FnConv) {
		self.pipeline.push(f);
	}
	pub fn run(&self, mut data: Blob) -> Blob {
		for f in self.pipeline.iter() {
			data = (f.func)(data);
		}
		data
	}
	pub fn description(&self) -> String {
		let names: Vec<String> = self.pipeline.iter().map(|e| e.name.clone()).collect();
		names.join(", ")
	}
}

impl PartialEq for DataConverter {
	fn eq(&self, other: &Self) -> bool {
		self.description() == other.description()
	}
}

impl Eq for DataConverter {}
