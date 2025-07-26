use crate::types::Blob;
use anyhow::{Context, Result};
use brotli::{BrotliCompress, BrotliDecompress, enc::BrotliEncoderParams};
use std::io::Cursor;

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
pub fn compress_brotli(blob: &Blob) -> Result<Blob> {
	let params = BrotliEncoderParams {
		quality: 10, // Highest quality
		lgwin: 19,   // Window size
		size_hint: blob.len() as usize,
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
pub fn compress_brotli_fast(blob: &Blob) -> Result<Blob> {
	let params = BrotliEncoderParams {
		quality: 3, // Lower quality for faster compression
		lgwin: 16,  // Smaller window size
		size_hint: blob.len() as usize,
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
pub fn decompress_brotli(blob: &Blob) -> Result<Blob> {
	let mut cursor = Cursor::new(blob.as_slice());
	let mut decompressed_data = Vec::new();
	BrotliDecompress(&mut cursor, &mut decompressed_data).context("Failed to decompress data using Brotli")?;
	Ok(Blob::from(decompressed_data))
}

#[cfg(test)]
mod tests {
	use super::super::tests::generate_test_data;
	use super::*;

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
