use anyhow::{bail, Result};
use std::{fmt::Debug, sync::Arc};
use versatiles_core::{
	types::*,
	utils::{compress, decompress},
};

/// A structure representing a pipeline of conversions to be applied to a blob
#[derive(Clone)]
pub struct TileConverter {
	src_comp: TileCompression,
	dst_comp: TileCompression,
	force_recompress: bool,
	src_form: TileFormat,
	dst_form: TileFormat,
	convert_format: bool,
	recompress: bool,
}

#[allow(dead_code)]
impl TileConverter {
	pub fn new(
		src_form: TileFormat,
		src_comp: TileCompression,
		dst_form: TileFormat,
		dst_comp: TileCompression,
		force_recompress: bool,
	) -> TileConverter {
		let convert_format = src_form != dst_form;
		let recompress = force_recompress || (src_comp != dst_comp) || convert_format;
		TileConverter {
			src_comp,
			dst_comp,
			force_recompress,
			src_form,
			dst_form,
			convert_format,
			recompress,
		}
	}

	/// Return `true` if the `DataConverter` has an empty pipeline
	pub fn is_empty(&self) -> bool {
		!self.force_recompress && self.src_form == self.dst_form && self.src_comp == self.dst_comp
	}

	/// Constructs a new `DataConverter` instance that decompresses data using the specified compression algorithm.
	/// The `src_comp` parameter specifies the compression algorithm to use: `Compression::Uncompressed`, `Compression::Gzip`, or `Compression::Brotli`.
	pub fn new_decompressor(src_comp: TileCompression) -> TileConverter {
		TileConverter {
			src_comp,
			..Default::default()
		}
	}

	fn status(&self) -> Result<String> {
		use TileCompression::*;

		let mut parts = vec![];

		if self.recompress && self.src_comp != Uncompressed {
			parts.push(match self.src_comp {
				Gzip => "ungzip",
				Brotli => "unbrotli",
				_ => bail!("source compression must be Uncompressed, Gzip or Brotli"),
			});
		};

		if self.convert_format {
			parts.push(match self.src_form {
				TileFormat::AVIF => "from avif",
				TileFormat::JPG => "from jpg",
				TileFormat::PNG => "from png",
				TileFormat::WEBP => "from webp",
				_ => bail!("source format must be AVIF, JPG, PNG or WEBP"),
			});

			parts.push(match self.dst_form {
				TileFormat::AVIF => "to avif",
				TileFormat::JPG => "to jpg",
				TileFormat::PNG => "to png",
				TileFormat::WEBP => "to webp",
				_ => bail!("destination format must be AVIF, JPG, PNG or WEBP"),
			});
		}

		if self.recompress && self.dst_comp != Uncompressed {
			parts.push(match self.dst_comp {
				Gzip => "gzip",
				Brotli => "brotli",
				_ => bail!("destination compression must be Uncompressed, Gzip or Brotli"),
			});
		}

		Ok(parts.join(" -> "))
	}

	/// Runs the data through the pipeline of conversion functions and returns the result.
	pub fn process_blob(&self, mut blob: Blob) -> Result<Blob> {
		if self.recompress {
			blob = decompress(blob, &self.src_comp)?;
		}

		if self.convert_format {
			use versatiles_image::{avif, jpeg, png, webp};
			let image = match self.src_form {
				TileFormat::AVIF => avif::blob2image(&blob)?,
				TileFormat::JPG => jpeg::blob2image(&blob)?,
				TileFormat::PNG => png::blob2image(&blob)?,
				TileFormat::WEBP => webp::blob2image(&blob)?,
				_ => bail!("Reading tile format '{}' is not implemented", self.src_form),
			};

			let lossless = self.src_form == TileFormat::PNG;

			blob = match (self.dst_form, lossless) {
				(TileFormat::AVIF, true) => avif::image2blob_lossless(&image)?,
				(TileFormat::AVIF, false) => avif::image2blob(&image, None)?,
				(TileFormat::JPG, _) => jpeg::image2blob(&image, None)?,
				(TileFormat::PNG, _) => png::image2blob(&image)?,
				(TileFormat::WEBP, true) => webp::image2blob_lossless(&image)?,
				(TileFormat::WEBP, false) => webp::image2blob(&image, None)?,
				_ => bail!("Writing tile format '{}' is not implemented", self.dst_form),
			};
		}

		if self.recompress {
			blob = compress(blob, &self.dst_comp)?;
		}

		Ok(blob)
	}

	/// Runs a stream through the pipeline of conversion functions
	pub fn process_stream<'a>(&'a self, stream: TileStream<'a>) -> TileStream<'a> {
		let me = Arc::new(self.clone());
		stream.map_blob_parallel(move |blob| me.process_blob(blob))
	}
}

impl Default for TileConverter {
	/// Constructs a new `TileConverter` instance with an empty pipeline.
	fn default() -> Self {
		TileConverter::new(
			TileFormat::BIN,
			TileCompression::Uncompressed,
			TileFormat::BIN,
			TileCompression::Uncompressed,
			false,
		)
	}
}

impl Debug for TileConverter {
	/// Returns a string representation of the `TileConverter` instance.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"TileConverter( {} )",
			self.status().unwrap_or_else(|e| format!("ERROR: {e}"))
		)
	}
}

#[cfg(test)]
mod tests {
	use std::vec;

	use super::*;
	use anyhow::ensure;

	#[test]
	fn new_tile_recompressor() -> Result<()> {
		fn test(
			src_format: TileFormat,
			src_comp: TileCompression,
			dst_format: TileFormat,
			dst_comp: TileCompression,
			force_recompress: &bool,
			length: usize,
			description: &str,
		) -> Result<()> {
			let data_converter = TileConverter::new(src_format, src_comp, dst_format, dst_comp, *force_recompress);

			let status = data_converter.status()?;
			ensure!(
				status == description,
				"status is \"{status}\" but expected \"{description}\"",
			);

			let steps = if status.is_empty() {
				0
			} else {
				status.split(" -> ").count()
			};
			ensure!(
				steps == length,
				"number of steps is \"{steps}\" but expected \"{length}\""
			);

			Ok(())
		}

		use TileCompression::*;
		let compressions = vec![Uncompressed, Gzip, Brotli];
		let formats = vec![TileFormat::AVIF, TileFormat::JPG, TileFormat::PNG, TileFormat::WEBP];
		let forcing = vec![false, true];

		for f_in in &formats {
			for c_in in &compressions {
				for f_out in &formats {
					for c_out in &compressions {
						for force in &forcing {
							let mut s = vec![];
							if f_in != f_out {
								s.push(format!("from {f_in}"));
								s.push(format!("to {f_out}"));
							}

							if !s.is_empty() || c_in != c_out || *force {
								if c_in != &Uncompressed {
									s.insert(0, format!("un{c_in}"));
								}
								if c_out != &Uncompressed {
									s.push(format!("{c_out}"));
								}
							}

							let length = s.len();

							test(*f_in, *c_in, *f_out, *c_out, force, length, &s.join(" -> "))?;
						}
					}
				}
			}
		}

		Ok(())
	}
}
