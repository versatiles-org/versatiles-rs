use crate::Result;

use super::{compress::*, image::*, Blob, Compression};
use clap::ValueEnum;
use std::fmt::Debug;

/// A structure representing a function that converts a blob to another blob
struct FnConv {
	func: fn(Blob) -> Result<Blob>,
	name: String,
}

impl FnConv {
	/// Create a new `FnConv` from a function and a name
	fn new(func: fn(Blob) -> Result<Blob>, name: &str) -> FnConv {
		FnConv {
			func,
			name: name.to_owned(),
		}
	}

	/// Create an optional `FnConv` from a function and a name
	fn some(func: fn(Blob) -> Result<Blob>, name: &str) -> Option<FnConv> {
		Some(FnConv::new(func, name))
	}

	#[allow(dead_code)]
	fn get_function(&self) -> fn(Blob) -> Result<Blob> {
		self.func
	}

	#[allow(dead_code)]
	fn get_name(&self) -> &str {
		&self.name
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

/// A structure representing a pipeline of conversions to be applied to a blob
#[derive(Debug)]
pub struct DataConverter {
	pipeline: Vec<FnConv>,
}

impl DataConverter {
	/// Create a new empty `DataConverter`
	pub fn new_empty() -> DataConverter {
		DataConverter { pipeline: Vec::new() }
	}

	/// Return `true` if the `DataConverter` has an empty pipeline
	pub fn is_empty(&self) -> bool {
		self.pipeline.is_empty()
	}

	/// Create a new `DataConverter` for tile recompression from `src_form` and `src_comp` to `dst_form` and `dst_comp`
	/// with optional forced recompression
	pub fn new_tile_recompressor(
		src_form: &TileFormat, src_comp: &Compression, dst_form: &TileFormat, dst_comp: &Compression,
		force_recompress: bool,
	) -> DataConverter {
		let mut converter = DataConverter::new_empty();

		// Create a format converter function based on the source and destination formats.
		let format_converter_option: Option<FnConv> = if (src_form != dst_form) || force_recompress {
			use TileFormat::*;
			match (src_form, dst_form) {
				(PNG, JPG) => FnConv::some(|tile| -> Result<Blob> { img2jpg(&png2img(tile)?) }, "PNG->JPG"),
				(PNG, PNG) => FnConv::some(|tile| -> Result<Blob> { img2png(&png2img(tile)?) }, "PNG->PNG"),
				(PNG, WEBP) => FnConv::some(
					|tile| -> Result<Blob> { img2webplossless(&png2img(tile)?) },
					"PNG->WEBP",
				),

				(JPG, PNG) => FnConv::some(|tile| -> Result<Blob> { img2png(&jpg2img(tile)?) }, "JPG->PNG"),
				(JPG, WEBP) => FnConv::some(|tile| -> Result<Blob> { img2webp(&jpg2img(tile)?) }, "JPG->WEBP"),

				(WEBP, JPG) => FnConv::some(|tile| -> Result<Blob> { img2jpg(&webp2img(tile)?) }, "WEBP->JPG"),
				(WEBP, PNG) => FnConv::some(|tile| -> Result<Blob> { img2png(&webp2img(tile)?) }, "WEBP->PNG"),

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

		// Push the necessary conversion functions to the converter pipeline.
		if (src_comp == dst_comp) && !force_recompress {
			if let Some(format_converter) = format_converter_option {
				converter.push(format_converter)
			}
		} else {
			use Compression::*;
			match src_comp {
				None => {}
				Gzip => converter.push(FnConv::new(decompress_gzip, "decompress_gzip")),
				Brotli => converter.push(FnConv::new(decompress_brotli, "decompress_brotli")),
			}
			if let Some(format_converter) = format_converter_option {
				converter.push(format_converter)
			}
			match dst_comp {
				None => {}
				Gzip => converter.push(FnConv::new(compress_gzip, "compress_gzip")),
				Brotli => converter.push(FnConv::new(compress_brotli, "compress_brotli")),
			}
		};

		converter
	}
	/// Constructs a new `DataConverter` instance that compresses data using the specified compression algorithm.
	/// The `dst_comp` parameter specifies the compression algorithm to use: `Compression::Uncompressed`, `Compression::Gzip`, or `Compression::Brotli`.
	pub fn new_compressor(dst_comp: &Compression) -> DataConverter {
		let mut converter = DataConverter::new_empty();

		match dst_comp {
			// If uncompressed, do nothing
			Compression::None => {}
			// If gzip, add the gzip compression function to the pipeline
			Compression::Gzip => converter.push(FnConv::new(compress_gzip, "compress_gzip")),
			// If brotli, add the brotli compression function to the pipeline
			Compression::Brotli => converter.push(FnConv::new(compress_brotli, "compress_brotli")),
		}

		converter
	}

	/// Constructs a new `DataConverter` instance that decompresses data using the specified compression algorithm.
	/// The `src_comp` parameter specifies the compression algorithm to use: `Compression::Uncompressed`, `Compression::Gzip`, or `Compression::Brotli`.
	pub fn new_decompressor(src_comp: &Compression) -> DataConverter {
		let mut converter = DataConverter::new_empty();

		match src_comp {
			// If uncompressed, do nothing
			Compression::None => {}
			// If gzip, add the gzip decompression function to the pipeline
			Compression::Gzip => converter.push(FnConv::new(decompress_gzip, "decompress_gzip")),
			// If brotli, add the brotli decompression function to the pipeline
			Compression::Brotli => converter.push(FnConv::new(decompress_brotli, "decompress_brotli")),
		}

		converter
	}
	/// Adds a new conversion function to the pipeline.
	fn push(&mut self, f: FnConv) {
		self.pipeline.push(f);
	}

	/// Runs the data through the pipeline of conversion functions and returns the result.
	pub fn run(&self, mut data: Blob) -> Result<Blob> {
		for f in self.pipeline.iter() {
			data = (f.func)(data)?;
		}
		Ok(data)
	}

	/// Returns a string describing the pipeline of conversion functions.
	pub fn description(&self) -> String {
		let names: Vec<String> = self.pipeline.iter().map(|e| e.name.clone()).collect();
		names.join(", ")
	}
}

/// Implements the `PartialEq` trait for the `DataConverter` struct.
/// This function returns true if the `description` method of both `DataConverter` instances returns the same value.
impl PartialEq for DataConverter {
	fn eq(&self, other: &Self) -> bool {
		self.description() == other.description()
	}
}

/// Implements the `Eq` trait for the `DataConverter` struct.
/// This trait is used in conjunction with `PartialEq` to provide a total equality relation for `DataConverter` instances.
impl Eq for DataConverter {}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{Blob, Compression, DataConverter, TileFormat};

	#[test]
	fn test_new() {
		let fn_conv = FnConv::new(|x| Ok(x), "test_fn_conv");
		assert_eq!(fn_conv.name, "test_fn_conv");
	}

	#[test]
	fn test_new_empty() {
		let data_converter = DataConverter::new_empty();
		assert_eq!(data_converter.pipeline.len(), 0);
	}

	#[test]
	fn test_is_empty() {
		let data_converter = DataConverter::new_empty();
		assert!(data_converter.is_empty());
	}

	#[test]
	fn test_new_tile_recompressor() {
		let src_form = TileFormat::PNG;
		let src_comp = Compression::Gzip;
		let dst_form = TileFormat::JPG;
		let dst_comp = Compression::Brotli;
		let force_recompress = false;
		let data_converter =
			DataConverter::new_tile_recompressor(&src_form, &src_comp, &dst_form, &dst_comp, force_recompress);
		assert_eq!(data_converter.pipeline.len(), 3);
	}

	// Test function for the `FnConv` struct
	#[test]
	fn test_fn_conv() {
		// Create a test `FnConv` instance
		let test_fn = FnConv::new(|blob| Ok(blob), "test");
		// Check the name of the `FnConv` instance
		assert_eq!(test_fn.get_name(), "test");

		// Check the function of the `FnConv` instance

		let func = test_fn.get_function();
		let vec = vec![1, 2, 3];
		assert_eq!(func(Blob::from(&vec)).unwrap().as_vec(), vec);
	}

	// Test function for the `DataConverter` struct
	#[test]
	fn test_data_converter() {
		// Create a test `DataConverter` instance
		let test_converter = DataConverter::new_tile_recompressor(
			&TileFormat::PNG,
			&Compression::Gzip,
			&TileFormat::JPG,
			&Compression::Brotli,
			true,
		);

		// Check if the converter is not empty
		assert!(!test_converter.is_empty());

		// Check if the converter has the correct number of conversion functions in the pipeline
		assert_eq!(test_converter.pipeline.len(), 3);

		// Check if the first function in the pipeline is the `decompress_gzip` function
		assert_eq!(test_converter.pipeline[0].name, "decompress_gzip");

		// Check if the second function in the pipeline is the `PNG->JPG` format conversion function
		assert_eq!(test_converter.pipeline[1].name, "PNG->JPG");

		// Check if the third function in the pipeline is the `compress_brotli` function
		assert_eq!(test_converter.pipeline[2].name, "compress_brotli");
	}
}
