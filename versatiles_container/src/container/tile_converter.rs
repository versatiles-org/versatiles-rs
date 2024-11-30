use anyhow::Result;
use itertools::Itertools;
use std::{
	fmt::{self, Debug},
	sync::Arc,
};
use versatiles_core::{types::*, utils::*};

#[derive(Clone, Debug)]
enum FnConv {
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
			FnConv::UnGzip => decompress_gzip(&blob),
			FnConv::UnBrotli => decompress_brotli(&blob),
			FnConv::Gzip => compress_gzip(&blob),
			FnConv::Brotli => compress_brotli(&blob),
		}
	}
}

/// A structure representing a pipeline of conversions to be applied to a blob
#[derive(Clone)]
pub struct TileConverter {
	pipeline: Arc<Vec<FnConv>>,
}

#[allow(dead_code)]
impl TileConverter {
	/// Create a new empty `DataConverter`
	pub fn new_empty() -> TileConverter {
		TileConverter {
			pipeline: Arc::new(Vec::new()),
		}
	}

	/// Return `true` if the `DataConverter` has an empty pipeline
	pub fn is_empty(&self) -> bool {
		self.pipeline.is_empty()
	}

	/// Create a new `DataConverter` for tile recompression from `src_form` and `src_comp` to `dst_form` and `dst_comp`
	/// with optional forced recompression
	#[allow(unused_variables)]
	pub fn new_tile_recompressor(
		src_comp: &TileCompression,
		dst_comp: &TileCompression,
		force_recompress: bool,
	) -> Result<TileConverter> {
		let mut converter = TileConverter::new_empty();

		// Push the necessary conversion functions to the converter pipeline.
		if force_recompress || (src_comp != dst_comp) {
			use TileCompression::*;
			match src_comp {
				Uncompressed => {}
				Gzip => converter.push(FnConv::UnGzip),
				Brotli => converter.push(FnConv::UnBrotli),
			}
			match dst_comp {
				Uncompressed => {}
				Gzip => converter.push(FnConv::Gzip),
				Brotli => converter.push(FnConv::Brotli),
			}
		};

		Ok(converter)
	}

	/// Constructs a new `DataConverter` instance that decompresses data using the specified compression algorithm.
	/// The `src_comp` parameter specifies the compression algorithm to use: `Compression::Uncompressed`, `Compression::Gzip`, or `Compression::Brotli`.
	pub fn new_decompressor(src_comp: &TileCompression) -> TileConverter {
		use TileCompression::*;
		let mut converter = TileConverter::new_empty();

		match src_comp {
			// If uncompressed, do nothing
			Uncompressed => {}
			// If gzip, add the gzip decompression function to the pipeline
			Gzip => converter.push(FnConv::UnGzip),
			// If brotli, add the brotli decompression function to the pipeline
			Brotli => converter.push(FnConv::UnBrotli),
		}

		converter
	}

	/// Adds a new conversion function to the pipeline.
	fn push(&mut self, f: FnConv) {
		Arc::get_mut(&mut self.pipeline).unwrap().push(f);
	}

	/// Runs the data through the pipeline of conversion functions and returns the result.
	pub fn process_blob(&self, mut blob: Blob) -> Result<Blob> {
		for f in self.pipeline.iter() {
			blob = f.run(blob)?;
		}
		Ok(blob)
	}

	/// Runs a stream through the pipeline of conversion functions
	pub fn process_stream<'a>(&'a self, stream: TileStream<'a>) -> TileStream<'a> {
		let pipeline = self.pipeline.clone();
		stream.map_blob_parallel(move |mut blob| {
			for f in pipeline.iter() {
				blob = f.run(blob).unwrap();
			}
			blob
		})
	}

	/// Returns a string describing the pipeline of conversion functions.
	pub fn as_string(&self) -> String {
		let names: Vec<String> = self.pipeline.iter().map(|f| f.to_string()).collect();
		names.join(",").to_lowercase()
	}
}

/// Implements the `PartialEq` trait for the `DataConverter` struct.
/// This function returns true if the `description` method of both `DataConverter` instances returns the same value.
impl PartialEq for TileConverter {
	fn eq(&self, other: &Self) -> bool {
		self.as_string() == other.as_string()
	}
}

impl fmt::Debug for TileConverter {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.pipeline.iter().map(|f| f.to_string()).join(", "))
	}
}

/// Implements the `Eq` trait for the `DataConverter` struct.
/// This trait is used in conjunction with `PartialEq` to provide a total equality relation for `DataConverter` instances.
impl Eq for TileConverter {}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::ensure;

	#[test]
	fn new_empty() {
		let data_converter = TileConverter::new_empty();
		assert_eq!(data_converter.pipeline.len(), 0);
	}

	#[test]
	fn is_empty() {
		let data_converter = TileConverter::new_empty();
		assert!(data_converter.is_empty());
	}

	#[test]
	fn new_tile_recompressor() {
		fn test(
			src_comp: &TileCompression,
			dst_comp: &TileCompression,
			force_recompress: &bool,
			length: usize,
			description: &str,
		) -> Result<()> {
			let data_converter =
				TileConverter::new_tile_recompressor(src_comp, dst_comp, *force_recompress)?;

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

		use TileCompression::*;
		let compressions = vec![Uncompressed, Gzip, Brotli];
		let forcing = vec![false, true];

		for c_in in &compressions {
			for c_out in &compressions {
				for force in &forcing {
					let mut s = format!("{},{}", decomp(c_in), comp(c_out));

					if !force {
						s = s.replace("ungzip,gzip", "");
						s = s.replace("unbrotli,brotli", "");
					}
					s = s.replace(",,", ",");
					s = s.strip_prefix(',').unwrap_or(&s).to_string();
					s = s.strip_suffix(',').unwrap_or(&s).to_string();

					let length = if s.is_empty() {
						0
					} else {
						s.split(',').count()
					};
					let message = format!("{c_in:?}->{c_out:?} {force}");

					let result = test(c_in, c_out, force, length, &s);

					assert!(
						result.is_ok(),
						"error for {message}: {}",
						result.err().unwrap()
					);
				}
			}
		}

		fn decomp(compression: &TileCompression) -> &str {
			match compression {
				Uncompressed => "",
				Gzip => "ungzip",
				Brotli => "unbrotli",
			}
		}

		fn comp(compression: &TileCompression) -> &str {
			match compression {
				Uncompressed => "",
				Gzip => "gzip",
				Brotli => "brotli",
			}
		}
	}
}
