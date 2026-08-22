use std::io::{Read, copy};

use anyhow::{Context, Result};
use flate2::bufread::{GzDecoder, GzEncoder};
use versatiles_derive::context;

use super::super::{LimitedWriter, max_decompressed_bytes};
use crate::Blob;

/// Compresses data using Gzip with highest quality settings.
///
/// # Arguments
///
/// * `blob` - The data blob to compress.
///
/// # Returns
///
/// * `Ok(Blob)` containing the Gzip-compressed data.
/// * `Err(anyhow::Error)` if compression fails.
///
/// # Errors
///
/// * If the Gzip compression process fails.
#[context("Compressing blob ({} bytes) using Gzip with highest quality settings", blob.len())]
pub fn compress_gzip(blob: &Blob) -> Result<Blob> {
	let mut encoder = GzEncoder::new(blob.as_slice(), flate2::Compression::best());
	let mut compressed_data = Vec::new();
	encoder
		.read_to_end(&mut compressed_data)
		.context("Failed to compress data using Gzip")?;
	Ok(Blob::from(compressed_data))
}

/// Compresses data using Gzip with faster settings.
///
/// This variant uses lower compression level for faster compression at the expense of compression ratio.
///
/// **Note:** This function is provided for direct use by callers who need to prioritize
/// compression speed over compression ratio. The [`optimize_compression`](super::super::optimize_compression)
/// function uses [`compress_gzip`] internally for maximum compression ratio.
///
/// # Arguments
///
/// * `blob` - The data blob to compress.
///
/// # Returns
///
/// * `Ok(Blob)` containing the Gzip-compressed data.
/// * `Err(anyhow::Error)` if compression fails.
///
/// # Errors
///
/// * If the Gzip compression process fails.
#[context("Compressing blob ({} bytes) using Gzip with fast compression settings", blob.len())]
pub fn compress_gzip_fast(blob: &Blob) -> Result<Blob> {
	let mut encoder = GzEncoder::new(blob.as_slice(), flate2::Compression::fast());
	let mut compressed_data = Vec::new();
	encoder
		.read_to_end(&mut compressed_data)
		.context("Failed to compress data using Gzip (fast)")?;
	Ok(Blob::from(compressed_data))
}

/// Decompresses data that was compressed using Gzip.
///
/// # Arguments
///
/// * `blob` - The Gzip-compressed data blob.
///
/// # Returns
///
/// * `Ok(Blob)` containing the decompressed data.
/// * `Err(anyhow::Error)` if decompression fails.
///
/// # Errors
///
/// * If the Gzip decompression process fails.
#[context("Decompressing blob ({} bytes) using Gzip", blob.len())]
pub fn decompress_gzip(blob: &Blob) -> Result<Blob> {
	decompress_gzip_bounded(blob, max_decompressed_bytes())
}

/// Decompress into a sink bounded by `limit` bytes (`0` = unbounded).
///
/// Split out so the bound itself is testable without touching the process
/// environment. See [`super::super::limit`] for why the bound exists.
fn decompress_gzip_bounded(blob: &Blob, limit: u64) -> Result<Blob> {
	let mut decoder = GzDecoder::new(blob.as_slice());
	let mut writer = LimitedWriter::new(limit);
	copy(&mut decoder, &mut writer).context("Failed to decompress data using Gzip")?;
	Ok(Blob::from(writer.into_vec()))
}

#[cfg(test)]
mod tests {
	/// A megabyte of zeroes compresses to a couple of kilobytes; decompressing
	/// it stops at the limit instead of running to completion. This is the shape
	/// of a decompression bomb, and the reason the bound exists.
	#[test]
	fn decompression_stops_at_the_limit() -> Result<()> {
		let bomb = compress_gzip(&Blob::from(vec![0u8; 1_000_000]))?;
		assert!(
			bomb.len() < 100_000,
			"the test bomb did not compress: {} bytes",
			bomb.len()
		);

		let error = decompress_gzip_bounded(&bomb, 64 * 1024).unwrap_err();
		assert!(
			format!("{error:#}").contains("exceeds the limit"),
			"unexpected error: {error:#}"
		);

		// A limit that fits the data, and 0 for no limit at all, both decompress fully.
		assert_eq!(decompress_gzip_bounded(&bomb, 2_000_000)?.len(), 1_000_000);
		assert_eq!(decompress_gzip_bounded(&bomb, 0)?.len(), 1_000_000);

		Ok(())
	}

	use super::{super::super::test_utils::generate_test_data, *};

	#[test]
	fn should_compress_and_decompress_gzip_correctly() -> Result<()> {
		let data = generate_test_data(100_000);
		let compressed = compress_gzip(&data)?;
		let decompressed = decompress_gzip(&compressed)?;
		assert_eq!(data, decompressed, "Gzip compression and decompression failed");
		Ok(())
	}

	#[test]
	fn should_compress_and_decompress_gzip_fast_correctly() -> Result<()> {
		let data = generate_test_data(100_000);
		let compressed = compress_gzip_fast(&data)?;
		let decompressed = decompress_gzip(&compressed)?;
		assert_eq!(data, decompressed, "Fast Gzip compression and decompression failed");
		Ok(())
	}
}
