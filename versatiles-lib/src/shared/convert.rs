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
#[cfg(feature = "full")]
use anyhow::bail;
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
	#[cfg(feature = "full")]
	Jpg2Png,
	#[cfg(feature = "full")]
	Jpg2Webp,
	//Png2Avif,
	#[cfg(feature = "full")]
	Png2Jpg,
	#[cfg(feature = "full")]
	Png2Png,
	#[cfg(feature = "full")]
	Png2Webplossless,
	//Webp2Avif,
	#[cfg(feature = "full")]
	Webp2Jpg,
	#[cfg(feature = "full")]
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
	fn run(&self, blob: Blob) -> Result<Blob> {
		match self {
			#[cfg(feature = "full")]
			FnConv::Png2Jpg => img2jpg(&png2img(&blob)?),
			#[cfg(feature = "full")]
			FnConv::Png2Png => img2png(&png2img(&blob)?),
			#[cfg(feature = "full")]
			FnConv::Png2Webplossless => img2webplossless(&png2img(&blob)?),
			#[cfg(feature = "full")]
			FnConv::Jpg2Png => img2png(&jpg2img(&blob)?),
			#[cfg(feature = "full")]
			FnConv::Jpg2Webp => img2webp(&jpg2img(&blob)?),
			#[cfg(feature = "full")]
			FnConv::Webp2Jpg => img2jpg(&webp2img(&blob)?),
			#[cfg(feature = "full")]
			FnConv::Webp2Png => img2png(&webp2img(&blob)?),
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
			FnConv::UnGzip => decompress_gzip(blob),
			FnConv::UnBrotli => decompress_brotli(blob),
			FnConv::Gzip => compress_gzip(blob),
			FnConv::Brotli => compress_brotli(blob),
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
	#[allow(unused_variables)]
	pub fn new_tile_recompressor(
		src_form: &TileFormat, src_comp: &Compression, dst_form: &TileFormat, dst_comp: &Compression,
		force_recompress: bool,
	) -> Result<DataConverter> {
		let mut converter = DataConverter::new_empty();

		// Create a format converter function based on the source and destination formats.
		#[cfg(not(feature = "full"))]
		let format_converter_option = None;

		#[cfg(feature = "full")]
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
						bail!("no conversion implemented for {:?} -> {:?}", src_form, dst_form);
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
		} else if let Some(format_converter) = format_converter_option {
			converter.push(format_converter)
		};

		Ok(converter)
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
		names.join(",").to_lowercase()
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
	use anyhow::{ensure, Result};

	#[cfg(feature = "full")]
	use crate::shared::{
		compare_images,
		create_image_rgb,
		//avif2img,
		img2jpg,
		img2png,
		img2webp,
		jpg2img,
		png2img,
		webp2img,
	};

	use crate::shared::{
		Compression::{self, *},
		DataConverter,
		TileFormat::{self, *},
	};

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
			src_form: &TileFormat, src_comp: &Compression, dst_form: &TileFormat, dst_comp: &Compression,
			force_recompress: &bool, length: usize, description: &str,
		) -> Result<()> {
			let data_converter =
				DataConverter::new_tile_recompressor(src_form, src_comp, dst_form, dst_comp, *force_recompress)?;

			ensure!(
				data_converter.as_string() == description,
				"description is \"{}\" but expected \"{}\"",
				data_converter.as_string(),
				description
			);

			ensure!(
				data_converter.pipeline.len() == length,
				"length is \"{}\" but expected \"{}\"",
				data_converter.pipeline.len(),
				length
			);

			Ok(())
		}

		let image_formats = vec![JPG, PNG, WEBP, PBF];
		let compressions = vec![None, Gzip, Brotli];
		let forcing = vec![false, true];

		for f_in in &image_formats {
			for c_in in &compressions {
				for f_out in &image_formats {
					for c_out in &compressions {
						for force in &forcing {
							let mut s = format!("{},{}2{},{}", decomp(c_in), form(f_in), form(f_out), comp(c_out));

							s = s.replace("png2webp", "png2webplossless");
							s = s.replace("jpg2jpg,", "");
							s = s.replace("webp2webp,", "");
							s = s.replace("pbf2pbf,", "");
							if !force {
								s = s.replace("png2png,", "");
								s = s.replace("ungzip,gzip", "");
								s = s.replace("unbrotli,brotli", "");
							}
							s = s.replace(",,", ",");
							s = s.strip_prefix(",").unwrap_or(&s).to_string();
							s = s.strip_suffix(",").unwrap_or(&s).to_string();

							#[cfg(not(feature = "full"))]
							if s.contains('2') {
								// if we don't use crate image, ignore image conversion
								continue;
							}

							let length = if s.len() == 0 { 0 } else { s.split(',').count() };
							let message = format!("{f_in:?},{c_in:?}->{f_out:?},{c_out:?} {force}");

							let result = test(f_in, c_in, f_out, c_out, force, length, &s);

							if is_image(f_in) == is_image(f_out) {
								assert!(result.is_ok(), "error for {message}: {}", result.err().unwrap());
							} else {
								assert!(result.is_err(), "error for {message}: should throw error");
							}
						}
					}
				}
			}
		}

		fn decomp(compression: &Compression) -> &str {
			match compression {
				None => "",
				Gzip => "ungzip",
				Brotli => "unbrotli",
			}
		}

		fn comp(compression: &Compression) -> &str {
			match compression {
				None => "",
				Gzip => "gzip",
				Brotli => "brotli",
			}
		}

		fn form(format: &TileFormat) -> &str {
			match format {
				AVIF => "avif",
				BIN => "bin",
				GEOJSON => "geojson",
				JPG => "jpg",
				JSON => "json",
				PBF => "pbf",
				PNG => "png",
				SVG => "svg",
				TOPOJSON => "topojson",
				WEBP => "webp",
			}
		}

		fn is_image(format: &TileFormat) -> bool {
			match format {
				AVIF => true,
				JPG => true,
				PNG => true,
				WEBP => true,

				BIN => false,
				GEOJSON => false,
				JSON => false,
				PBF => false,
				SVG => false,
				TOPOJSON => false,
			}
		}
	}

	#[test]
	#[cfg(feature = "full")]
	fn convert_images() -> Result<()> {
		let formats = vec![
			//AVIF,
			JPG, PNG, WEBP,
		];

		for src_form in formats.iter() {
			for dst_form in formats.iter() {
				let image1 = create_image_rgb();
				let blob1 = match src_form {
					//AVIF => img2avif(&image1)?,
					JPG => img2jpg(&image1)?,
					PNG => img2png(&image1)?,
					WEBP => img2webp(&image1)?,
					_ => panic!("unsupported format {src_form:?}"),
				};

				let data_converter = DataConverter::new_tile_recompressor(
					&src_form,
					&Compression::None,
					&dst_form,
					&Compression::None,
					true,
				)?;

				let blob2 = data_converter.process_blob(blob1)?;

				let image2 = match dst_form {
					//AVIF => avif2img(blob)?,
					JPG => jpg2img(&blob2)?,
					PNG => png2img(&blob2)?,
					WEBP => webp2img(&blob2)?,
					_ => panic!("not allowed"),
				};

				assert_eq!(image2.width(), 256, "image should be 256 pixels wide");
				assert_eq!(image2.height(), 256, "image should be 256 pixels high");

				compare_images(image1, image2, 7);
			}
		}
		Ok(())
	}
}
