use std::io::Cursor;

use anyhow::{Context, Result};
use brotli::{BrotliCompress, BrotliDecompress, enc::BrotliEncoderParams};
use versatiles_derive::context;

use super::super::{LimitedWriter, max_decompressed_bytes};
use crate::Blob;

/// Compresses data using Brotli.
///
/// # Arguments
///
/// * `blob` - The data blob to compress.
///
/// # Returns
///
/// * `Ok(Blob)` containing the Brotli-compressed data.
/// * `Err(anyhow::Error)` if compression fails.
///
/// # Errors
///
/// * If the Brotli compression process fails.
#[context("Compressing blob ({} bytes) using Brotli with highest quality settings", blob.len())]
pub fn compress_brotli(blob: &Blob) -> Result<Blob> {
	let params = BrotliEncoderParams {
		quality: 10, // Highest quality
		lgwin: 19,   // Window size
		size_hint: usize::try_from(blob.len())?,
		..Default::default()
	};
	let mut input = Cursor::new(blob.as_slice());
	let mut output = Vec::new();
	BrotliCompress(&mut input, &mut output, &params).context("Failed to compress data using Brotli")?;
	Ok(Blob::from(output))
}

/// Compresses data using Brotli with faster settings.
///
/// This variant uses lower quality settings for faster compression at the expense of compression ratio.
///
/// **Note:** This function is provided for direct use by callers who need to prioritize
/// compression speed over compression ratio. The [`optimize_compression`](super::super::optimize_compression)
/// function uses [`compress_brotli`] internally for maximum compression ratio.
///
/// # Arguments
///
/// * `blob` - The data blob to compress.
///
/// # Returns
///
/// * `Ok(Blob)` containing the Brotli-compressed data.
/// * `Err(anyhow::Error)` if compression fails.
///
/// # Errors
///
/// * If the Brotli compression process fails.
#[context("Compressing blob ({} bytes) using Brotli with fast compression settings", blob.len())]
pub fn compress_brotli_fast(blob: &Blob) -> Result<Blob> {
	let params = BrotliEncoderParams {
		quality: 3, // Lower quality for faster compression
		lgwin: 16,  // Smaller window size
		size_hint: usize::try_from(blob.len())?,
		..Default::default()
	};
	let mut input = Cursor::new(blob.as_slice());
	let mut output = Vec::new();
	BrotliCompress(&mut input, &mut output, &params).context("Failed to compress data using Brotli (fast)")?;
	Ok(Blob::from(output))
}

/// Decompresses data that was compressed using Brotli.
///
/// # Arguments
///
/// * `blob` - The Brotli-compressed data blob.
///
/// # Returns
///
/// * `Ok(Blob)` containing the decompressed data.
/// * `Err(anyhow::Error)` if decompression fails.
///
/// # Errors
///
/// * If the Brotli decompression process fails.
#[context("Decompressing blob ({} bytes) using Brotli", blob.len())]
pub fn decompress_brotli(blob: &Blob) -> Result<Blob> {
	decompress_brotli_bounded(blob, max_decompressed_bytes())
}

/// Decompress into a sink bounded by `limit` bytes (`0` = unbounded).
///
/// Split out so the bound itself is testable without touching the process
/// environment. See [`super::super::limit`] for why the bound exists.
fn decompress_brotli_bounded(blob: &Blob, limit: u64) -> Result<Blob> {
	let mut cursor = Cursor::new(blob.as_slice());
	let mut writer = LimitedWriter::new(limit);
	BrotliDecompress(&mut cursor, &mut writer).context("Failed to decompress data using Brotli")?;
	Ok(Blob::from(writer.into_vec()))
}

#[cfg(test)]
mod tests {
	/// A megabyte of zeroes compresses to a couple of kilobytes; decompressing
	/// it stops at the limit instead of running to completion. This is the shape
	/// of a decompression bomb, and the reason the bound exists.
	#[test]
	fn decompression_stops_at_the_limit() -> Result<()> {
		let bomb = compress_brotli_fast(&Blob::from(vec![0u8; 1_000_000]))?;
		assert!(
			bomb.len() < 100_000,
			"the test bomb did not compress: {} bytes",
			bomb.len()
		);

		let error = decompress_brotli_bounded(&bomb, 64 * 1024).unwrap_err();
		assert!(
			format!("{error:#}").contains("exceeds the limit"),
			"unexpected error: {error:#}"
		);

		// A limit that fits the data, and 0 for no limit at all, both decompress fully.
		assert_eq!(decompress_brotli_bounded(&bomb, 2_000_000)?.len(), 1_000_000);
		assert_eq!(decompress_brotli_bounded(&bomb, 0)?.len(), 1_000_000);

		Ok(())
	}

	use super::{super::super::test_utils::generate_test_data, *};

	#[test]
	fn should_compress_and_decompress_brotli_correctly() -> Result<()> {
		let data = generate_test_data(10_000);
		let compressed = compress_brotli(&data)?;
		let decompressed = decompress_brotli(&compressed)?;
		assert_eq!(data, decompressed, "Brotli compression and decompression failed");
		Ok(())
	}

	#[test]
	fn should_compress_and_decompress_brotli_fast_correctly() -> Result<()> {
		let data = generate_test_data(10_000);
		let compressed = compress_brotli_fast(&data)?;
		let decompressed = decompress_brotli(&compressed)?;
		assert_eq!(data, decompressed, "Fast Brotli compression and decompression failed");
		Ok(())
	}
}
