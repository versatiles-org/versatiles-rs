#![allow(non_snake_case)]

use crate::create_error;

use super::{Blob, Result};
use brotli::{enc::BrotliEncoderParams, BrotliCompress, BrotliDecompress};
use clap::ValueEnum;
use enumset::{EnumSet, EnumSetType};
use flate2::bufread::{GzDecoder, GzEncoder};
use std::io::{Cursor, Read};

/// Enum representing possible compression algorithms
#[derive(Debug, EnumSetType, ValueEnum)]
pub enum Compression {
	None,
	Gzip,
	Brotli,
}

#[derive(Debug, PartialEq)]
pub struct TargetCompression {
	compressions: EnumSet<Compression>,
	best_compression: bool,
}
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

pub fn optimize_compression(data: Blob, input: &Compression, target: TargetCompression) -> Result<(Blob, Compression)> {
	if target.compressions.is_empty() {
		return create_error!("no compression allowed");
	}

	if !target.best_compression && target.compressions.contains(*input) {
		return Ok((data, *input));
	}

	match input {
		Compression::None => {
			if target.compressions.contains(Compression::Brotli) {
				return Ok((compress_brotli(data)?, Compression::Brotli));
			}

			if target.compressions.contains(Compression::Gzip) {
				return Ok((compress_gzip(data)?, Compression::Gzip));
			}

			Ok((data, Compression::None))
		}
		Compression::Gzip => {
			if target.compressions.contains(Compression::Gzip) {
				return Ok((data, Compression::Gzip));
			}
			let data = decompress_gzip(data)?;

			if target.compressions.contains(Compression::Brotli) {
				return Ok((compress_brotli(data)?, Compression::Brotli));
			}

			Ok((data, Compression::None))
		}
		Compression::Brotli => {
			if target.compressions.contains(Compression::Brotli) {
				return Ok((data, Compression::Brotli));
			}
			let data = decompress_brotli(data)?;

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
pub fn compress(data: Blob, compression: &Compression) -> Result<Blob> {
	match compression {
		Compression::None => Ok(data),
		Compression::Gzip => compress_gzip(data),
		Compression::Brotli => compress_brotli(data),
	}
}

/// Decompresses data based on the given compression algorithm
///
/// # Arguments
///
/// * `data` - The blob of data to decompress
/// * `compression` - The compression algorithm used for compression
pub fn decompress(data: Blob, compression: &Compression) -> Result<Blob> {
	match compression {
		Compression::None => Ok(data),
		Compression::Gzip => decompress_gzip(data),
		Compression::Brotli => decompress_brotli(data),
	}
}

/// Compresses data using gzip
///
/// # Arguments
///
/// * `data` - The blob of data to compress
pub fn compress_gzip(data: Blob) -> Result<Blob> {
	let mut result: Vec<u8> = Vec::new();
	GzEncoder::new(data.as_slice(), flate2::Compression::best()).read_to_end(&mut result)?;

	Ok(Blob::from(result))
}

/// Decompresses data that was compressed using gzip
///
/// # Arguments
///
/// * `data` - The blob of data to decompress
pub fn decompress_gzip(data: Blob) -> Result<Blob> {
	let mut result: Vec<u8> = Vec::new();
	GzDecoder::new(data.as_slice()).read_to_end(&mut result)?;

	Ok(Blob::from(result))
}

/// Compresses data using Brotli
///
/// # Arguments
///
/// * `data` - The blob of data to compress
pub fn compress_brotli(data: Blob) -> Result<Blob> {
	let params = BrotliEncoderParams {
		quality: 11,
		size_hint: data.len(),
		..Default::default()
	};
	let mut cursor = Cursor::new(data.as_slice());
	let mut result: Vec<u8> = Vec::new();
	BrotliCompress(&mut cursor, &mut result, &params)?;

	Ok(Blob::from(result))
}

/// Decompresses data that was compressed using Brotli
///
/// # Arguments
///
/// * `data` - The blob of data to decompress
pub fn decompress_brotli(data: Blob) -> Result<Blob> {
	let mut cursor = Cursor::new(data.as_slice());
	let mut result: Vec<u8> = Vec::new();
	BrotliDecompress(&mut cursor, &mut result)?;

	Ok(Blob::from(result))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	/// Verify that the Brotli compression and decompression functions work correctly.
	fn verify_brotli() -> Result<()> {
		// Generate random data.
		let data1 = random_data(100000);

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

	/// Generate random binary data of a specified size.
	fn random_data(size: usize) -> Blob {
		let mut vec: Vec<u8> = Vec::new();
		vec.resize(size, 0);
		(0..size).for_each(|i| {
			vec[i] = (((i as f64 + 1.78123).cos() * 6_513_814_013_423.454).fract() * 256f64) as u8;
		});

		Blob::from(vec)
	}
}
