use std::io::Cursor;

use anyhow::Result;
use versatiles_derive::context;

use super::super::{LimitedWriter, max_decompressed_bytes};
use crate::Blob;

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
	decompress_zstd_bounded(blob, max_decompressed_bytes())
}

/// Decompress into a sink bounded by `limit` bytes (`0` = unbounded).
///
/// Split out so the bound itself is testable without touching the process
/// environment. See [`super::super::limit`] for why the bound exists.
fn decompress_zstd_bounded(blob: &Blob, limit: u64) -> Result<Blob> {
	let mut writer = LimitedWriter::new(limit);
	zstd::stream::copy_decode(Cursor::new(blob.as_slice()), &mut writer)?;
	Ok(Blob::from(writer.into_vec()))
}

#[cfg(test)]
mod tests {
	/// A megabyte of zeroes compresses to a couple of kilobytes; decompressing
	/// it stops at the limit instead of running to completion. This is the shape
	/// of a decompression bomb, and the reason the bound exists.
	#[test]
	fn decompression_stops_at_the_limit() -> Result<()> {
		let bomb = compress_zstd(&Blob::from(vec![0u8; 1_000_000]))?;
		assert!(
			bomb.len() < 100_000,
			"the test bomb did not compress: {} bytes",
			bomb.len()
		);

		let error = decompress_zstd_bounded(&bomb, 64 * 1024).unwrap_err();
		assert!(
			format!("{error:#}").contains("exceeds the limit"),
			"unexpected error: {error:#}"
		);

		// A limit that fits the data, and 0 for no limit at all, both decompress fully.
		assert_eq!(decompress_zstd_bounded(&bomb, 2_000_000)?.len(), 1_000_000);
		assert_eq!(decompress_zstd_bounded(&bomb, 0)?.len(), 1_000_000);

		Ok(())
	}

	use super::{super::super::test_utils::generate_test_data, *};

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
