use crate::Blob;
use anyhow::Result;
use std::io::Cursor;
use versatiles_derive::context;

/// Compresses data using Zstd with highest quality settings.
///
/// # Arguments
///
/// * `blob` - The data blob to compress.
///
/// # Returns
///
/// * `Ok(Blob)` containing the Zstd-compressed data.
/// * `Err(anyhow::Error)` if compression fails.
///
/// # Errors
///
/// * If the Zstd compression process fails.
#[context("Compressing blob ({} bytes) using Zstd with highest quality settings", blob.len())]
pub fn compress_zstd(blob: &Blob) -> Result<Blob> {
	// Zstd compression level range: 1-22, default is 3, max quality is 22
	let compressed = zstd::encode_all(Cursor::new(blob.as_slice()), 19)?;
	Ok(Blob::from(compressed))
}

/// Compresses data using Zstd with faster settings.
///
/// This variant uses lower compression level for faster compression at the expense of compression ratio.
///
/// **Note:** This function is provided for direct use by callers who need to prioritize
/// compression speed over compression ratio. The [`optimize_compression`](super::super::optimize_compression)
/// function uses [`compress_zstd`] internally for maximum compression ratio.
///
/// # Arguments
///
/// * `blob` - The data blob to compress.
///
/// # Returns
///
/// * `Ok(Blob)` containing the Zstd-compressed data.
/// * `Err(anyhow::Error)` if compression fails.
///
/// # Errors
///
/// * If the Zstd compression process fails.
#[context("Compressing blob ({} bytes) using Zstd with fast compression settings", blob.len())]
pub fn compress_zstd_fast(blob: &Blob) -> Result<Blob> {
	// Use level 3 for faster compression
	let compressed = zstd::encode_all(Cursor::new(blob.as_slice()), 3)?;
	Ok(Blob::from(compressed))
}

/// Decompresses data that was compressed using Zstd.
///
/// # Arguments
///
/// * `blob` - The Zstd-compressed data blob.
///
/// # Returns
///
/// * `Ok(Blob)` containing the decompressed data.
/// * `Err(anyhow::Error)` if decompression fails.
///
/// # Errors
///
/// * If the Zstd decompression process fails.
#[context("Decompressing blob ({} bytes) using Zstd", blob.len())]
pub fn decompress_zstd(blob: &Blob) -> Result<Blob> {
	let decompressed = zstd::decode_all(Cursor::new(blob.as_slice()))?;
	Ok(Blob::from(decompressed))
}

#[cfg(test)]
mod tests {
	use super::super::super::test_utils::generate_test_data;
	use super::*;

	#[test]
	fn should_compress_and_decompress_zstd_correctly() -> Result<()> {
		let data = generate_test_data(100_000);
		let compressed = compress_zstd(&data)?;
		let decompressed = decompress_zstd(&compressed)?;
		assert_eq!(data, decompressed, "Zstd compression and decompression failed");
		Ok(())
	}

	#[test]
	fn should_compress_and_decompress_zstd_fast_correctly() -> Result<()> {
		let data = generate_test_data(100_000);
		let compressed = compress_zstd_fast(&data)?;
		let decompressed = decompress_zstd(&compressed)?;
		assert_eq!(data, decompressed, "Fast Zstd compression and decompression failed");
		Ok(())
	}
}
