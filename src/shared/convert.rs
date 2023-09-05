#[cfg(feature = "full")]
use super::image::{img2jpg, img2png, img2webp, img2webplossless, jpg2img, png2img, webp2img};
use super::{compress_brotli, compress_gzip, decompress_brotli, decompress_gzip, Blob, Compression, Result};
use crate::{containers::TileStream, create_error};
#[cfg(feature = "full")]
use clap::ValueEnum;
use futures_util::StreamExt;
use itertools::Itertools;
use std::{
	fmt::{self, Debug},
	sync::Arc,
};

#[derive(Clone, Debug)]
enum FnConv {
	Png2Jpg,
	Png2Png,
	Png2Webplossless,
	Jpg2Png,
	Jpg2Webp,
	Webp2Jpg,
	Webp2Png,
	UnGzip,
	UnBrotli,
	Gzip,
	Brotli,
}

impl fmt::Display for FnConv {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:?}", self)
	}
}

/// A structure representing a function that converts a blob to another blob

impl FnConv {
	#[allow(unreachable_patterns)]
	fn run(&self, tile: Blob) -> Result<Blob> {
		match self {
			#[cfg(feature = "full")]
			FnConv::Png2Jpg => img2jpg(png2img(tile)?),
			#[cfg(feature = "full")]
			FnConv::Png2Png => img2png(png2img(tile)?),
			#[cfg(feature = "full")]
			FnConv::Png2Webplossless => img2webplossless(png2img(tile)?),
			#[cfg(feature = "full")]
			FnConv::Jpg2Png => img2png(jpg2img(tile)?),
			#[cfg(feature = "full")]
			FnConv::Jpg2Webp => img2webp(jpg2img(tile)?),
			#[cfg(feature = "full")]
			FnConv::Webp2Jpg => img2jpg(webp2img(tile)?),
			#[cfg(feature = "full")]
			FnConv::Webp2Png => img2png(webp2img(tile)?),

			FnConv::UnGzip => decompress_gzip(tile),
			FnConv::UnBrotli => decompress_brotli(tile),
			FnConv::Gzip => compress_gzip(tile),
			FnConv::Brotli => compress_brotli(tile),

			_ => create_error!("{self:?} is not supported"),
		}
	}
}

// Enum representing supported tile formats
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "full", derive(ValueEnum))]
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
#[derive(Clone)]
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
				(PNG, JPG) => Some(FnConv::Png2Jpg),
				(PNG, PNG) => Some(FnConv::Png2Png),
				(PNG, WEBP) => Some(FnConv::Png2Webplossless),

				(JPG, PNG) => Some(FnConv::Jpg2Png),
				(JPG, WEBP) => Some(FnConv::Jpg2Webp),

				(WEBP, JPG) => Some(FnConv::Webp2Jpg),
				(WEBP, PNG) => Some(FnConv::Webp2Png),

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
				Gzip => converter.push(FnConv::UnGzip),
				Brotli => converter.push(FnConv::UnBrotli),
			}
			if let Some(format_converter) = format_converter_option {
				converter.push(format_converter)
			}
			match dst_comp {
				None => {}
				Gzip => converter.push(FnConv::Gzip),
				Brotli => converter.push(FnConv::Brotli),
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
			Compression::Gzip => converter.push(FnConv::Gzip),
			// If brotli, add the brotli compression function to the pipeline
			Compression::Brotli => converter.push(FnConv::Brotli),
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
			Compression::Gzip => converter.push(FnConv::UnGzip),
			// If brotli, add the brotli decompression function to the pipeline
			Compression::Brotli => converter.push(FnConv::UnBrotli),
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
			blob = f.run(blob)?;
		}
		Ok(blob)
	}

	#[allow(dead_code)]
	/// Runs a stream through the pipeline of conversion functions
	pub fn process_stream<'a>(&'a self, stream: TileStream<'a>) -> TileStream<'a> {
		let pipeline = Arc::new(self.pipeline.clone());
		stream
			.map(move |(coord, mut blob)| {
				let pipeline = pipeline.clone();
				tokio::spawn(async move {
					for f in pipeline.iter() {
						blob = f.run(blob).unwrap();
					}
					(coord, blob)
				})
			})
			.buffer_unordered(num_cpus::get())
			.map(|r| r.unwrap())
			.boxed()
	}

	/// Returns a string describing the pipeline of conversion functions.
	pub fn description(&self) -> String {
		let names: Vec<String> = self.pipeline.iter().map(|f| f.to_string()).collect();
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

impl fmt::Debug for DataConverter {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.pipeline.iter().map(|f| f.to_string()).join(", "))
	}
}

/// Implements the `Eq` trait for the `DataConverter` struct.
/// This trait is used in conjunction with `PartialEq` to provide a total equality relation for `DataConverter` instances.
impl Eq for DataConverter {}

#[cfg(test)]
mod tests {
	use crate::shared::{
		Compression::{self, *},
		DataConverter,
		TileFormat::{self, *},
	};
	use std::panic::catch_unwind;

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
			assert_eq!(data_converter.description(), description);
			assert_eq!(data_converter.pipeline.len(), length);
			assert_eq!(data_converter, data_converter.clone());
		}

		assert!(catch_unwind(|| {
			test(PBF, Brotli, PNG, Brotli, false, 3, "hello3");
		})
		.is_err());

		assert!(catch_unwind(|| {
			test(PNG, None, PBF, Gzip, true, 3, "hello4");
		})
		.is_err());

		test(PBF, None, PBF, Brotli, false, 1, "Brotli");
		test(PNG, Gzip, PNG, Brotli, false, 2, "UnGzip, Brotli");
		test(PNG, None, PNG, None, false, 0, "");
		test(PNG, None, PNG, None, true, 1, "Png2Png");
		test(PNG, Gzip, PNG, Brotli, false, 2, "UnGzip, Brotli");
		test(PNG, Gzip, PNG, Brotli, true, 3, "UnGzip, Png2Png, Brotli");

		test(PNG, Gzip, JPG, Gzip, false, 1, "Png2Jpg");
		test(PNG, Brotli, PNG, Gzip, true, 3, "UnBrotli, Png2Png, Gzip");
		test(PNG, None, WEBP, None, false, 1, "Png2Webplossless");
		test(JPG, Gzip, PNG, None, true, 2, "UnGzip, Jpg2Png");
		test(JPG, Brotli, WEBP, None, false, 2, "UnBrotli, Jpg2Webp");
		test(WEBP, None, JPG, Brotli, true, 2, "Webp2Jpg, Brotli");
		test(WEBP, Gzip, PNG, Brotli, false, 3, "UnGzip, Webp2Png, Brotli");
		test(PNG, Brotli, WEBP, Gzip, true, 3, "UnBrotli, Png2Webplossless, Gzip");
		test(PNG, None, WEBP, Gzip, false, 2, "Png2Webplossless, Gzip");
	}
}
