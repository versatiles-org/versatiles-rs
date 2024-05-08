#[cfg(feature = "full")]
use super::image::{img2jpg, img2png, img2webp, img2webplossless, jpg2img, png2img, webp2img};
use super::{
	//avif2img,
	compress_brotli,
	compress_gzip,
	decompress_brotli,
	decompress_gzip,
	//img2avif,
	Blob,
	Compression,
	TileFormat,
};
use crate::containers::TilesStream;
use anyhow::Result;
use futures_util::StreamExt;
use itertools::Itertools;
use std::{
	fmt::{self, Debug},
	sync::Arc,
};

#[derive(Clone, Debug)]
enum FnConv {
	//	Avif2Jpg,
	//Avif2Png,
	//Avif2Webp,
	//Jpg2Avif,
	Jpg2Png,
	Jpg2Webp,
	//Png2Avif,
	Png2Jpg,
	Png2Png,
	Png2Webplossless,
	//Webp2Avif,
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
			//#[cfg(feature = "full")]
			//FnConv::Avif2Jpg => img2jpg(avif2img(tile)?),
			//#[cfg(feature = "full")]
			//FnConv::Avif2Png => img2png(avif2img(tile)?),
			//#[cfg(feature = "full")]
			//FnConv::Avif2Webp => img2webp(avif2img(tile)?),
			//#[cfg(feature = "full")]
			//FnConv::Jpg2Avif => img2avif(jpg2img(tile)?),
			//#[cfg(feature = "full")]
			//FnConv::Png2Avif => img2avif(png2img(tile)?),
			//#[cfg(feature = "full")]
			//FnConv::Webp2Avif => img2avif(webp2img(tile)?),
			FnConv::UnGzip => decompress_gzip(tile),
			FnConv::UnBrotli => decompress_brotli(tile),
			FnConv::Gzip => compress_gzip(tile),
			FnConv::Brotli => compress_brotli(tile),
		}
	}
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
				//(AVIF, AVIF) => Some(),
				//(AVIF, JPG) => Some(FnConv::Avif2Jpg),
				//(AVIF, PNG) => Some(FnConv::Avif2Png),
				//(AVIF, WEBP) => Some(FnConv::Avif2Webp),

				//(JPG, AVIF) => Some(FnConv::Jpg2Avif),
				//(JPG, JPG) => Some(),
				(JPG, PNG) => Some(FnConv::Jpg2Png),
				(JPG, WEBP) => Some(FnConv::Jpg2Webp),

				//(PNG, AVIF) => Some(FnConv::Png2Avif),
				(PNG, JPG) => Some(FnConv::Png2Jpg),
				(PNG, PNG) => Some(FnConv::Png2Png),
				(PNG, WEBP) => Some(FnConv::Png2Webplossless),

				//(WEBP, AVIF) => Some(FnConv::Webp2Avif),
				(WEBP, JPG) => Some(FnConv::Webp2Jpg),
				(WEBP, PNG) => Some(FnConv::Webp2Png),
				//(WEBP, WEBP) => Some(),
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
		if force_recompress || (src_comp != dst_comp) || format_converter_option.is_some() {
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
		} else {
			if let Some(format_converter) = format_converter_option {
				converter.push(format_converter)
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
	pub fn process_stream<'a>(&'a self, stream: TilesStream<'a>) -> TilesStream<'a> {
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
	pub fn as_string(&self) -> String {
		let names: Vec<String> = self.pipeline.iter().map(|f| f.to_string()).collect();
		names.join(", ")
	}
}

/// Implements the `PartialEq` trait for the `DataConverter` struct.
/// This function returns true if the `description` method of both `DataConverter` instances returns the same value.
impl PartialEq for DataConverter {
	fn eq(&self, other: &Self) -> bool {
		self.as_string() == other.as_string()
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
		//avif2img,
		jpg2img,
		png2img,
		webp2img,
		Blob,
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
			assert_eq!(
				data_converter.as_string(),
				description,
				"description error in {src_form:?},{src_comp:?}->{dst_form:?},{dst_comp:?}"
			);
			assert_eq!(
				data_converter.pipeline.len(),
				length,
				"length error in {src_form:?},{src_comp:?}->{dst_form:?},{dst_comp:?}"
			);
		}

		assert!(catch_unwind(|| {
			test(PBF, Brotli, PNG, Brotli, false, 3, "hello3");
		})
		.is_err());

		assert!(catch_unwind(|| {
			test(PNG, None, PBF, Gzip, true, 3, "hello4");
		})
		.is_err());

		test(PBF, Gzip, PBF, Gzip, false, 0, "");
		test(PBF, None, PBF, Brotli, false, 1, "Brotli");
		test(PNG, Gzip, PNG, Brotli, false, 2, "UnGzip, Brotli");
		test(PNG, None, PNG, None, false, 0, "");
		test(PNG, None, PNG, None, true, 1, "Png2Png");
		test(PNG, Gzip, PNG, Gzip, true, 3, "UnGzip, Png2Png, Gzip");
		test(PNG, Gzip, PNG, Gzip, false, 0, "");
		test(PNG, Gzip, PNG, Brotli, false, 2, "UnGzip, Brotli");
		test(PNG, Gzip, PNG, Brotli, true, 3, "UnGzip, Png2Png, Brotli");

		test(PNG, Gzip, JPG, Gzip, false, 3, "UnGzip, Png2Jpg, Gzip");
		test(PNG, Brotli, PNG, Gzip, true, 3, "UnBrotli, Png2Png, Gzip");
		test(PNG, None, WEBP, None, false, 1, "Png2Webplossless");
		test(JPG, Gzip, PNG, None, true, 2, "UnGzip, Jpg2Png");
		test(JPG, Brotli, WEBP, None, false, 2, "UnBrotli, Jpg2Webp");
		test(WEBP, None, JPG, Brotli, true, 2, "Webp2Jpg, Brotli");
		test(WEBP, Gzip, PNG, Brotli, false, 3, "UnGzip, Webp2Png, Brotli");
		test(PNG, Brotli, WEBP, Gzip, true, 3, "UnBrotli, Png2Webplossless, Gzip");
		test(PNG, None, WEBP, Gzip, false, 2, "Png2Webplossless, Gzip");
	}

	#[test]
	fn convert_images() {
		use crate::containers::{
			//MOCK_BYTES_AVIF,
			MOCK_BYTES_JPG,
			MOCK_BYTES_PNG,
			MOCK_BYTES_WEBP,
		};

		let formats = vec![
			//AVIF,
			JPG, PNG, WEBP,
		];
		let comp = Compression::None;
		for src_form in formats.iter() {
			for dst_form in formats.iter() {
				let (mut blob, should_size) = match src_form {
					//AVIF => (Blob::from(MOCK_BYTES_AVIF.to_vec()), ""),
					JPG => (Blob::from(MOCK_BYTES_JPG.to_vec()), ""),
					PNG => (Blob::from(MOCK_BYTES_PNG.to_vec()), ""),
					WEBP => (Blob::from(MOCK_BYTES_WEBP.to_vec()), ""),
					_ => panic!("not allowed"),
				};

				let data_converter = DataConverter::new_tile_recompressor(&src_form, &comp, &dst_form, &comp, false);

				blob = data_converter.process_blob(blob).unwrap();

				println!("{:?}", blob);

				let image = match dst_form {
					//AVIF => avif2img(blob).unwrap(),
					JPG => jpg2img(blob).unwrap(),
					PNG => png2img(blob).unwrap(),
					WEBP => webp2img(blob).unwrap(),
					_ => panic!("not allowed"),
				};

				let size = format!("{}x{}", image.width(), image.height());
				assert_eq!(size, should_size, "should have the correct size for {src_form:?}");
			}
		}
	}
}
