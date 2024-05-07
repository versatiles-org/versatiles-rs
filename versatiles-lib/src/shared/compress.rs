#![allow(non_snake_case, dead_code)]

use super::{Blob, Compression};
use anyhow::{bail, Result};
use brotli::{enc::BrotliEncoderParams, BrotliCompress, BrotliDecompress};
use enumset::EnumSet;
use flate2::bufread::{GzDecoder, GzEncoder};
use std::io::{Cursor, Read};

#[derive(Debug, PartialEq)]
pub struct TargetCompression {
	compressions: EnumSet<Compression>,
	best_compression: bool,
}
#[allow(dead_code)]
impl TargetCompression {
	pub fn from_set(compressions: EnumSet<Compression>) -> Self {
		TargetCompression {
			compressions,
			best_compression: true,
		}
	}
	pub fn from(compression: Compression) -> Self {
		Self::from_set(EnumSet::only(compression))
	}
	pub fn from_none() -> Self {
		Self::from(Compression::None)
	}
	pub fn set_best_compression(&mut self, best_compression: bool) {
		self.best_compression = best_compression;
	}
	pub fn contains(&self, compression: Compression) -> bool {
		self.compressions.contains(compression)
	}
	pub fn insert(&mut self, compression: Compression) {
		self.compressions.insert(compression);
	}
}

#[allow(dead_code)]
pub fn optimize_compression(blob: Blob, input: &Compression, target: TargetCompression) -> Result<(Blob, Compression)> {
	if target.compressions.is_empty() {
		bail!("no compression allowed");
	}

	if !target.best_compression && target.compressions.contains(*input) {
		return Ok((blob, *input));
	}

	match input {
		Compression::None => {
			if target.compressions.contains(Compression::Brotli) {
				return Ok((compress_brotli(blob)?, Compression::Brotli));
			}

			if target.compressions.contains(Compression::Gzip) {
				return Ok((compress_gzip(blob)?, Compression::Gzip));
			}

			Ok((blob, Compression::None))
		}
		Compression::Gzip => {
			if target.compressions.contains(Compression::Brotli) {
				return Ok((compress_brotli(decompress_gzip(blob)?)?, Compression::Brotli));
			}

			if target.compressions.contains(Compression::Gzip) {
				return Ok((blob, Compression::Gzip));
			}

			Ok((decompress_gzip(blob)?, Compression::None))
		}
		Compression::Brotli => {
			if target.compressions.contains(Compression::Brotli) {
				return Ok((blob, Compression::Brotli));
			}
			let data = decompress_brotli(blob)?;

			if target.compressions.contains(Compression::Gzip) {
				return Ok((compress_gzip(data)?, Compression::Gzip));
			}

			Ok((data, Compression::None))
		}
	}
}

/// Compresses data based on the given compression algorithm
///
/// # Arguments
///
/// * `data` - The blob of data to compress
/// * `compression` - The compression algorithm to use
pub fn compress(blob: Blob, compression: &Compression) -> Result<Blob> {
	match compression {
		Compression::None => Ok(blob),
		Compression::Gzip => compress_gzip(blob),
		Compression::Brotli => compress_brotli(blob),
	}
}

/// Decompresses data based on the given compression algorithm
///
/// # Arguments
///
/// * `data` - The blob of data to decompress
/// * `compression` - The compression algorithm used for compression
pub fn decompress(blob: Blob, compression: &Compression) -> Result<Blob> {
	match compression {
		Compression::None => Ok(blob),
		Compression::Gzip => decompress_gzip(blob),
		Compression::Brotli => decompress_brotli(blob),
	}
}

/// Compresses data using gzip
///
/// # Arguments
///
/// * `data` - The blob of data to compress
pub fn compress_gzip(blob: Blob) -> Result<Blob> {
	let mut result: Vec<u8> = Vec::new();
	GzEncoder::new(blob.as_slice(), flate2::Compression::best()).read_to_end(&mut result)?;
	Ok(Blob::from(result))
}

/// Decompresses data that was compressed using gzip
///
/// # Arguments
///
/// * `data` - The blob of data to decompress
pub fn decompress_gzip(blob: Blob) -> Result<Blob> {
	let mut result: Vec<u8> = Vec::new();
	GzDecoder::new(blob.as_slice()).read_to_end(&mut result)?;
	Ok(Blob::from(result))
}

/// Compresses data using Brotli
///
/// # Arguments
///
/// * `data` - The blob of data to compress
pub fn compress_brotli(blob: Blob) -> Result<Blob> {
	let params = BrotliEncoderParams {
		quality: 10, // smallest
		lgwin: 19,   // smallest
		size_hint: blob.len(),
		..Default::default()
	};
	let mut input = Cursor::new(blob.as_slice());
	let mut output: Vec<u8> = Vec::new();
	BrotliCompress(&mut input, &mut output, &params)?;

	Ok(Blob::from(output))
}

/// Decompresses data that was compressed using Brotli
///
/// # Arguments
///
/// * `data` - The blob of data to decompress
pub fn decompress_brotli(blob: Blob) -> Result<Blob> {
	let mut cursor = Cursor::new(blob.as_slice());
	let mut result: Vec<u8> = Vec::new();
	BrotliDecompress(&mut cursor, &mut result)?;
	Ok(Blob::from(result))
}

#[cfg(test)]
mod tests {
	use super::*;
	use enumset::enum_set;

	#[test]
	/// Verify that the Brotli compression and decompression functions work correctly.
	fn verify_brotli() -> Result<()> {
		// Generate random data.
		let data1 = random_data(10000);

		// Compress and then decompress the data.
		let data2 = decompress_brotli(compress_brotli(data1.clone())?)?;

		// Check that the original and decompressed data match.
		assert_eq!(data1, data2);

		Ok(())
	}

	#[test]
	/// Verify that the Gzip compression and decompression functions work correctly.
	fn verify_gzip() -> Result<()> {
		// Generate random data.
		let data1 = random_data(100000);

		// Compress and then decompress the data.
		let data2 = decompress_gzip(compress_gzip(data1.clone())?)?;

		// Check that the original and decompressed data match.
		assert_eq!(data1, data2);

		Ok(())
	}

	#[test]
	/// Test optimize_compression and try all combinations
	fn test_optimize_compression() -> Result<()> {
		let blob = random_data(100);
		let blob_gzip = compress_gzip(blob.clone())?;
		let blob_brotli = compress_brotli(blob.clone())?;

		let test = |compression_in: Compression,
		            compressions_out: EnumSet<Compression>,
		            best_compression: bool,
		            compression_exp: Compression|
		 -> Result<()> {
			let target = TargetCompression {
				compressions: compressions_out,
				best_compression,
			};
			let data_in = match compression_in {
				Compression::None => blob.clone(),
				Compression::Gzip => blob_gzip.clone(),
				Compression::Brotli => blob_brotli.clone(),
			};
			let data_exp = match compression_exp {
				Compression::None => blob.clone(),
				Compression::Gzip => blob_gzip.clone(),
				Compression::Brotli => blob_brotli.clone(),
			};
			let (data_res, compression_res) = optimize_compression(data_in, &compression_in, target)?;
			assert_eq!(
				compression_res, compression_exp,
				"compressing from {compression_in:?} to {compressions_out:?} using best compression ({best_compression}) should result {compression_exp:?} and not {compression_res:?}"
			);

			assert_eq!(data_res, data_exp);
			Ok(())
		};

		let N = Compression::None;
		let G = Compression::Gzip;
		let B = Compression::Brotli;

		let sN = enum_set!(Compression::None);
		let sG = enum_set!(Compression::Gzip);
		let sB = enum_set!(Compression::Brotli);
		let sNG = enum_set!(Compression::None | Compression::Gzip);
		let sNB = enum_set!(Compression::None | Compression::Brotli);
		let sNGB = enum_set!(Compression::None | Compression::Gzip | Compression::Brotli);

		test(N, sN, true, N)?;
		test(N, sG, true, G)?;
		test(N, sB, true, B)?;
		test(N, sNG, true, G)?;
		test(N, sNB, true, B)?;
		test(N, sNGB, true, B)?;

		test(G, sN, true, N)?;
		test(G, sG, true, G)?;
		test(G, sB, true, B)?;
		test(G, sNG, true, G)?;
		test(G, sNB, true, B)?;
		test(G, sNGB, true, B)?;

		test(B, sN, true, N)?;
		test(B, sG, true, G)?;
		test(B, sB, true, B)?;
		test(B, sNG, true, G)?;
		test(B, sNB, true, B)?;
		test(B, sNGB, true, B)?;

		test(N, sN, false, N)?;
		test(N, sG, false, G)?;
		test(N, sB, false, B)?;
		test(N, sNG, false, N)?;
		test(N, sNB, false, N)?;
		test(N, sNGB, false, N)?;

		test(G, sN, false, N)?;
		test(G, sG, false, G)?;
		test(G, sB, false, B)?;
		test(G, sNG, false, G)?;
		test(G, sNB, false, B)?;
		test(G, sNGB, false, G)?;

		test(B, sN, false, N)?;
		test(B, sG, false, G)?;
		test(B, sB, false, B)?;
		test(B, sNG, false, G)?;
		test(B, sNB, false, B)?;
		test(B, sNGB, false, B)?;

		Ok(())
	}

	/// Generate random binary data of a specified size.
	fn random_data(size: usize) -> Blob {
		let mut vec: Vec<u8> = vec![0; size];
		(0..size).for_each(|i| {
			vec[i] = (((i as f64 + 1.78123).cos() * 6_513_814_013_423.454).fract() * 256f64) as u8;
		});

		Blob::from(vec)
	}
}
