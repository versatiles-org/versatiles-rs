use crate::Blob;
use anyhow::{Context, Result};
use flate2::bufread::{GzDecoder, GzEncoder};
use std::io::Read;
use versatiles_derive::context;

/// Compresses data using Gzip.
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
#[context("Compressing blob with algorithm: Gzip")]
pub fn compress_gzip(blob: &Blob) -> Result<Blob> {
	let mut encoder = GzEncoder::new(blob.as_slice(), flate2::Compression::best());
	let mut compressed_data = Vec::new();
	encoder
		.read_to_end(&mut compressed_data)
		.context("Failed to compress data using Gzip")?;
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
#[context("Decompressing data using Gzip")]
pub fn decompress_gzip(blob: &Blob) -> Result<Blob> {
	let mut decoder = GzDecoder::new(blob.as_slice());
	let mut decompressed_data = Vec::new();
	decoder
		.read_to_end(&mut decompressed_data)
		.context("Failed to decompress data using Gzip")?;
	Ok(Blob::from(decompressed_data))
}

#[cfg(test)]
mod tests {
	use super::super::tests::generate_test_data;
	use super::*;

	#[test]
	fn should_compress_and_decompress_gzip_correctly() -> Result<()> {
		let data = generate_test_data(100_000);
		let compressed = compress_gzip(&data)?;
		let decompressed = decompress_gzip(&compressed)?;
		assert_eq!(data, decompressed, "Gzip compression and decompression failed");
		Ok(())
	}
}
