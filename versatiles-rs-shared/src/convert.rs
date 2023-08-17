use super::{compress::*, image::*, Blob, Compression, Result, TileCoord3};
use clap::ValueEnum;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use std::fmt::Debug;

type FnConvType = fn(Blob) -> Result<Blob>;

#[derive(Clone)]
/// A structure representing a function that converts a blob to another blob
struct FnConv {
	func: FnConvType,
	name: String,
}

impl FnConv {
	/// Create a new `FnConv` from a function and a name
	fn new(func: FnConvType, name: &str) -> FnConv {
		FnConv {
			func,
			name: name.to_owned(),
		}
	}

	/// Create an optional `FnConv` from a function and a name
	fn some(func: FnConvType, name: &str) -> Option<FnConv> {
		Some(FnConv::new(func, name))
	}

	// Getter function for testing the function field
	#[cfg(test)]
	fn get_function(&self) -> FnConvType {
		self.func
	}

	// Getter function for testing the name field
	#[cfg(test)]
	fn get_name(&self) -> &str {
		&self.name
	}
}

impl Debug for FnConv {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.name)
	}
}

// Enum representing supported tile formats
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
#[derive(Debug, Clone)]
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
				(PNG, JPG) => FnConv::some(|tile| -> Result<Blob> { img2jpg(png2img(tile)?) }, "PNG->JPG"),
				(PNG, PNG) => FnConv::some(|tile| -> Result<Blob> { img2png(png2img(tile)?) }, "PNG->PNG"),
				(PNG, WEBP) => FnConv::some(|tile| -> Result<Blob> { img2webplossless(png2img(tile)?) }, "PNG->WEBP"),

				(JPG, PNG) => FnConv::some(|tile| -> Result<Blob> { img2png(jpg2img(tile)?) }, "JPG->PNG"),
				(JPG, WEBP) => FnConv::some(|tile| -> Result<Blob> { img2webp(jpg2img(tile)?) }, "JPG->WEBP"),

				(WEBP, JPG) => FnConv::some(|tile| -> Result<Blob> { img2jpg(webp2img(tile)?) }, "WEBP->JPG"),
				(WEBP, PNG) => FnConv::some(|tile| -> Result<Blob> { img2png(webp2img(tile)?) }, "WEBP->PNG"),

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
	pub fn process_blob(&self, mut blob: Blob) -> Result<Blob> {
		for f in self.pipeline.iter() {
			blob = (f.func)(blob)?;
		}
		Ok(blob)
	}

	/// Runs a stream through the pipeline of conversion functions
	pub fn process_vec(&self, vec: Vec<(TileCoord3, Blob)>) -> Vec<(TileCoord3, Blob)> {
		let pipeline = self.pipeline.clone();
		vec.into_par_iter()
			.map(move |(coord, mut blob)| {
				for f in pipeline.iter() {
					blob = (f.func)(blob).unwrap();
				}
				(coord, blob)
			})
			.collect()
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
	use crate::{
		convert::FnConv,
		Blob,
		Compression::{self, *},
		DataConverter,
		TileFormat::{self, *},
	};
	use std::panic::catch_unwind;

	#[test]
	fn new() {
		let fn_conv = FnConv::new(|x| Ok(x.clone()), "test_fn_conv");
		assert_eq!(fn_conv.name, "test_fn_conv");
	}

	#[test]
	fn new_empty() {
		let data_converter = DataConverter::new_empty();
		assert_eq!(data_converter.pipeline.len(), 0);
	}

	#[test]
	fn is_empty() {
		let data_converter = DataConverter::new_empty();
		assert!(data_converter.is_empty());
	}

	#[test]
	fn new_tile_recompressor() {
		fn test(
			src_form: TileFormat, src_comp: Compression, dst_form: TileFormat, dst_comp: Compression,
			force_recompress: bool, length: usize, description: &str,
		) {
			let data_converter =
				DataConverter::new_tile_recompressor(&src_form, &src_comp, &dst_form, &dst_comp, force_recompress);
			assert_eq!(data_converter.description().replace("compress_", "_"), description);
			assert_eq!(data_converter.pipeline.len(), length);
			assert_eq!(data_converter, data_converter.clone());
		}

		assert!(catch_unwind(|| {
			test(PBF, Brotli, PNG, Brotli, false, 3, "hallo3");
		})
		.is_err());

		assert!(catch_unwind(|| {
			test(PNG, None, PBF, Gzip, true, 3, "hallo4");
		})
		.is_err());

		test(PBF, None, PBF, Brotli, false, 1, "_brotli");
		test(PNG, Gzip, PNG, Brotli, false, 2, "de_gzip, _brotli");
		test(PNG, None, PNG, None, false, 0, "");
		test(PNG, None, PNG, None, true, 1, "PNG->PNG");
		test(PNG, Gzip, PNG, Brotli, false, 2, "de_gzip, _brotli");
		test(PNG, Gzip, PNG, Brotli, true, 3, "de_gzip, PNG->PNG, _brotli");

		test(PNG, Gzip, JPG, Gzip, false, 1, "PNG->JPG");
		test(PNG, Brotli, PNG, Gzip, true, 3, "de_brotli, PNG->PNG, _gzip");
		test(PNG, None, WEBP, None, false, 1, "PNG->WEBP");
		test(JPG, Gzip, PNG, None, true, 2, "de_gzip, JPG->PNG");
		test(JPG, Brotli, WEBP, None, false, 2, "de_brotli, JPG->WEBP");
		test(WEBP, None, JPG, Brotli, true, 2, "WEBP->JPG, _brotli");
		test(WEBP, Gzip, PNG, Brotli, false, 3, "de_gzip, WEBP->PNG, _brotli");
		test(PNG, Brotli, WEBP, Gzip, true, 3, "de_brotli, PNG->WEBP, _gzip");
		test(PNG, None, WEBP, Gzip, false, 2, "PNG->WEBP, _gzip");
	}

	// Test function for the `FnConv` struct
	#[test]
	fn fn_conv() {
		// Create a test `FnConv` instance
		let test_fn = FnConv::new(|blob| Ok(blob.clone()), "test");

		// Check the name of the `FnConv` instance
		assert_eq!(test_fn.get_name(), "test");

		// Check the function of the `FnConv` instance
		let func = test_fn.get_function();
		let vec = vec![1, 2, 3];
		assert_eq!(func(Blob::from(&vec)).unwrap().as_vec(), vec);
	}
}
