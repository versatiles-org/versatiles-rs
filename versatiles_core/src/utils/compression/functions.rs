//! # Compression Module
//!
//! This module provides functionalities to compress and decompress data blobs
//! using various compression algorithms such as Gzip, Brotli, and Zstd. It also allows
//! optimizing compression based on target preferences and handling recompression.
//!
//! ## Features
//! - Compress and decompress data using Gzip, Brotli, and Zstd.
//! - Optimize compression based on target settings.
//! - Recompress data from one compression format to another.
//!
//! ## Usage
//! ```rust
//! use versatiles_core::{utils::*, *};
//!
//! let data = Blob::from(vec![1, 2, 3, 4, 5]);
//! let compressed = compress_gzip(&data)?;
//! let decompressed = decompress_gzip(&compressed)?;
//! assert_eq!(data, decompressed);
//! # Ok::<(), anyhow::Error>(())
//! ```
use super::{
	compression_goal::CompressionGoal,
	methods::{compress_brotli, compress_gzip, compress_zstd, decompress_brotli, decompress_gzip, decompress_zstd},
	target_compression::TargetCompression,
};
use crate::{Blob, TileCompression};
use anyhow::{Result, bail};
use versatiles_derive::context;

/// Optimizes the compression of a data blob based on the target compression settings.
///
/// This function attempts to compress or decompress the input blob to match the desired compression
/// settings. It ensures that the resulting blob adheres to the allowed compression algorithms and
/// the specified compression goal.
///
/// # Arguments
///
/// * `blob` - The input data blob to compress or decompress.
/// * `input_compression` - The current compression algorithm of the blob.
/// * `target` - The target compression settings.
///
/// # Returns
///
/// * `Ok((Blob, TileCompression))` containing the optimized blob and its compression algorithm.
/// * `Err(anyhow::Error)` if the optimization fails.
///
/// # Errors
///
/// * If no compression algorithms are allowed in the target.
/// * If 'Uncompressed' is not included in the allowed compressions.
/// * If decompression or compression operations fail.
#[context("Optimizing compression for blob with input compression: {input_compression:?} and target: {target:?}")]
pub fn optimize_compression(
	blob: Blob,
	input_compression: TileCompression,
	target: &TargetCompression,
) -> Result<(Blob, TileCompression)> {
	if target.compressions.is_empty() {
		bail!("At least one compression algorithm must be allowed");
	}

	if !target.compressions.contains(TileCompression::Uncompressed) {
		bail!("'Uncompressed' must always be supported");
	}

	use CompressionGoal::{IsIncompressible, UseBestCompression};

	// If the target is not seeking the best compression and the current compression is allowed,
	// retain the current compression.
	if target.compression_goal != UseBestCompression && target.compressions.contains(input_compression) {
		return Ok((blob, input_compression));
	}

	match input_compression {
		TileCompression::Uncompressed => {
			if target.compressions.contains(TileCompression::Zstd) {
				return Ok((compress_zstd(&blob)?, TileCompression::Zstd));
			}

			if target.compression_goal != IsIncompressible {
				if target.compressions.contains(TileCompression::Brotli) {
					return Ok((compress_brotli(&blob)?, TileCompression::Brotli));
				}

				if target.compressions.contains(TileCompression::Gzip) {
					return Ok((compress_gzip(&blob)?, TileCompression::Gzip));
				}
			}

			Ok((blob, TileCompression::Uncompressed))
		}
		TileCompression::Gzip => {
			if target.compression_goal != IsIncompressible && target.compressions.contains(TileCompression::Zstd) {
				let decompressed = decompress_gzip(&blob)?;
				let compressed_zstd = compress_zstd(&decompressed)?;
				return Ok((compressed_zstd, TileCompression::Zstd));
			}

			if target.compression_goal != IsIncompressible && target.compressions.contains(TileCompression::Brotli) {
				let decompressed = decompress_gzip(&blob)?;
				let compressed_brotli = compress_brotli(&decompressed)?;
				return Ok((compressed_brotli, TileCompression::Brotli));
			}

			if target.compressions.contains(TileCompression::Gzip) {
				return Ok((blob, TileCompression::Gzip));
			}

			// Fallback to Uncompressed if Gzip is not allowed
			let decompressed = decompress_gzip(&blob)?;
			Ok((decompressed, TileCompression::Uncompressed))
		}
		TileCompression::Brotli => {
			if target.compressions.contains(TileCompression::Brotli) {
				return Ok((blob, TileCompression::Brotli));
			}
			let decompressed = decompress_brotli(&blob)?;

			if target.compression_goal != IsIncompressible && target.compressions.contains(TileCompression::Zstd) {
				let compressed_zstd = compress_zstd(&decompressed)?;
				return Ok((compressed_zstd, TileCompression::Zstd));
			}

			if target.compression_goal != IsIncompressible && target.compressions.contains(TileCompression::Gzip) {
				let compressed_gzip = compress_gzip(&decompressed)?;
				return Ok((compressed_gzip, TileCompression::Gzip));
			}

			Ok((decompressed, TileCompression::Uncompressed))
		}
		TileCompression::Zstd => {
			if target.compressions.contains(TileCompression::Zstd) {
				return Ok((blob, TileCompression::Zstd));
			}

			// When seeking best compression and Brotli is allowed, convert Zstd to Brotli
			if target.compression_goal != IsIncompressible && target.compressions.contains(TileCompression::Brotli) {
				let decompressed = decompress_zstd(&blob)?;
				let compressed_brotli = compress_brotli(&decompressed)?;
				return Ok((compressed_brotli, TileCompression::Brotli));
			}

			let decompressed = decompress_zstd(&blob)?;

			if target.compression_goal != IsIncompressible && target.compressions.contains(TileCompression::Gzip) {
				let compressed_gzip = compress_gzip(&decompressed)?;
				return Ok((compressed_gzip, TileCompression::Gzip));
			}

			Ok((decompressed, TileCompression::Uncompressed))
		}
	}
}

/// Recompresses a data blob from one compression algorithm to another.
///
/// This function first decompresses the blob using the input compression algorithm and then
/// compresses it using the output compression algorithm.
///
/// # Arguments
///
/// * `blob` - The input data blob to recompress.
/// * `input_compression` - The current compression algorithm of the blob.
/// * `output_compression` - The desired compression algorithm.
///
/// # Returns
///
/// * `Ok(Blob)` containing the recompressed data.
/// * `Err(anyhow::Error)` if decompression or compression fails.
///
/// # Errors
///
/// * If decompression or compression operations fail.
#[context("Recompressing blob from {input_compression:?} to {output_compression:?}")]
pub fn recompress(blob: Blob, input_compression: TileCompression, output_compression: TileCompression) -> Result<Blob> {
	if input_compression == output_compression {
		return Ok(blob);
	}
	let decompressed = decompress(blob, input_compression)?;
	let recompressed = compress(decompressed, output_compression)?;
	Ok(recompressed)
}

/// Compresses data based on the specified compression algorithm.
///
/// # Arguments
///
/// * `blob` - The data blob to compress.
/// * `compression` - The compression algorithm to use.
///
/// # Returns
///
/// * `Ok(Blob)` containing the compressed data.
/// * `Err(anyhow::Error)` if compression fails.
///
/// # Errors
///
/// * If the specified compression algorithm is unsupported.
#[context("Compressing blob with algorithm: {compression:?}")]
pub fn compress(blob: Blob, compression: TileCompression) -> Result<Blob> {
	match compression {
		TileCompression::Uncompressed => Ok(blob),
		TileCompression::Gzip => compress_gzip(&blob),
		TileCompression::Brotli => compress_brotli(&blob),
		TileCompression::Zstd => compress_zstd(&blob),
	}
}

/// Decompresses data based on the specified compression algorithm.
///
/// # Arguments
///
/// * `blob` - The data blob to decompress.
/// * `compression` - The compression algorithm used for compression.
///
/// # Returns
///
/// * `Ok(Blob)` containing the decompressed data.
/// * `Err(anyhow::Error)` if decompression fails.
///
/// # Errors
///
/// * If the specified compression algorithm is unsupported.
#[context("Decompressing blob with algorithm: {compression:?}")]
pub fn decompress(blob: Blob, compression: TileCompression) -> Result<Blob> {
	match compression {
		TileCompression::Uncompressed => Ok(blob),
		TileCompression::Gzip => decompress_gzip(&blob),
		TileCompression::Brotli => decompress_brotli(&blob),
		TileCompression::Zstd => decompress_zstd(&blob),
	}
}

/// Decompresses a data reference based on the specified compression algorithm.
#[context("Decompressing blob ref with algorithm: {compression:?}")]
pub fn decompress_ref(blob: &Blob, compression: TileCompression) -> Result<Blob> {
	match compression {
		TileCompression::Uncompressed => Ok(blob.clone()),
		TileCompression::Gzip => decompress_gzip(blob),
		TileCompression::Brotli => decompress_brotli(blob),
		TileCompression::Zstd => decompress_zstd(blob),
	}
}

#[cfg(test)]
mod tests {
	use super::super::test_utils::generate_test_data;
	use super::*;
	use enumset::{EnumSet, enum_set};
	use rstest::{fixture, rstest};

	use CompressionGoal::*;
	use TileCompression::*;

	/// Fixture providing pre-compressed test blobs for all compression types.
	struct TestBlobs {
		original: Blob,
		gzip: Blob,
		brotli: Blob,
		zstd: Blob,
	}

	impl TestBlobs {
		fn get(&self, compression: TileCompression) -> Blob {
			match compression {
				Uncompressed => self.original.clone(),
				Gzip => self.gzip.clone(),
				Brotli => self.brotli.clone(),
				Zstd => self.zstd.clone(),
			}
		}
	}

	#[fixture]
	fn blobs() -> TestBlobs {
		let original = generate_test_data(100);
		TestBlobs {
			gzip: compress_gzip(&original).unwrap(),
			brotli: compress_brotli(&original).unwrap(),
			zstd: compress_zstd(&original).unwrap(),
			original,
		}
	}

	fn allowed(types: &[TileCompression]) -> EnumSet<TileCompression> {
		types.iter().copied().collect()
	}

	#[rstest]
	// Best compression: prefers Zstd, keeps Brotli if already Brotli
	#[case::best_from_uncompressed(Uncompressed, &[Uncompressed, Gzip, Brotli, Zstd], UseBestCompression, Zstd)]
	#[case::best_from_gzip(Gzip, &[Uncompressed, Gzip, Brotli, Zstd], UseBestCompression, Zstd)]
	#[case::best_from_brotli(Brotli, &[Uncompressed, Gzip, Brotli, Zstd], UseBestCompression, Brotli)]
	#[case::best_from_zstd(Zstd, &[Uncompressed, Gzip, Brotli, Zstd], UseBestCompression, Zstd)]
	// Fast compression: keeps current format if allowed
	#[case::fast_from_uncompressed(Uncompressed, &[Uncompressed, Gzip, Brotli, Zstd], UseFastCompression, Uncompressed)]
	#[case::fast_gzip_to_gzip(Gzip, &[Uncompressed, Gzip], UseFastCompression, Gzip)]
	#[case::fast_gzip_to_brotli(Gzip, &[Uncompressed, Brotli], UseFastCompression, Brotli)]
	#[case::fast_gzip_to_zstd(Gzip, &[Uncompressed, Zstd], UseFastCompression, Zstd)]
	#[case::fast_from_brotli(Brotli, &[Uncompressed, Gzip, Brotli, Zstd], UseFastCompression, Brotli)]
	#[case::fast_from_zstd(Zstd, &[Uncompressed, Gzip, Brotli, Zstd], UseFastCompression, Zstd)]
	// Incompressible: keeps current format
	#[case::incompressible_uncompressed(Uncompressed, &[Uncompressed], IsIncompressible, Uncompressed)]
	#[case::incompressible_gzip(Gzip, &[Uncompressed, Gzip], IsIncompressible, Gzip)]
	#[case::incompressible_brotli(Brotli, &[Uncompressed, Brotli], IsIncompressible, Brotli)]
	#[case::incompressible_zstd(Zstd, &[Uncompressed, Zstd], IsIncompressible, Zstd)]
	fn optimize_compression_scenarios(
		blobs: TestBlobs,
		#[case] input: TileCompression,
		#[case] allowed_types: &[TileCompression],
		#[case] goal: CompressionGoal,
		#[case] expected: TileCompression,
	) {
		let target = TargetCompression {
			compressions: allowed(allowed_types),
			compression_goal: goal,
		};

		let (result_blob, result_compression) = optimize_compression(blobs.get(input), input, &target).unwrap();

		assert_eq!(result_compression, expected);
		assert_eq!(result_blob, blobs.get(expected));
	}

	#[test]
	fn should_recompress_correctly() -> Result<()> {
		let original_data = generate_test_data(1_000);
		let gzip_data = compress_gzip(&original_data)?;
		let brotli_data = compress_brotli(&original_data)?;

		// Recompress Gzip to Brotli
		let recompressed = recompress(gzip_data.clone(), TileCompression::Gzip, TileCompression::Brotli)?;
		let decompressed = decompress_brotli(&recompressed)?;
		assert_eq!(original_data, decompressed);

		// Recompress Brotli to Gzip
		let recompressed = recompress(brotli_data.clone(), TileCompression::Brotli, TileCompression::Gzip)?;
		let decompressed = decompress_gzip(&recompressed)?;
		assert_eq!(original_data, decompressed);

		// Recompress Gzip to Gzip (no change)
		let recompressed = recompress(gzip_data.clone(), TileCompression::Gzip, TileCompression::Gzip)?;
		assert_eq!(recompressed, gzip_data);

		// Recompress Uncompressed to Gzip
		let recompressed = recompress(
			original_data.clone(),
			TileCompression::Uncompressed,
			TileCompression::Gzip,
		)?;
		let decompressed = decompress_gzip(&recompressed)?;
		assert_eq!(original_data, decompressed);

		Ok(())
	}

	#[test]
	fn should_handle_no_compression_correctly() -> Result<()> {
		let data = generate_test_data(500);
		let result = optimize_compression(
			data.clone(),
			TileCompression::Uncompressed,
			&TargetCompression::from(TileCompression::Uncompressed),
		)?;
		assert_eq!(result.0, data);
		assert_eq!(result.1, TileCompression::Uncompressed);
		Ok(())
	}

	#[test]
	fn should_fail_when_no_compressions_allowed() {
		let data = generate_test_data(100);
		let target = TargetCompression {
			compressions: EnumSet::empty(),
			compression_goal: CompressionGoal::UseBestCompression,
		};
		let result = optimize_compression(data, TileCompression::Uncompressed, &target);
		assert!(result.is_err());
	}

	#[test]
	fn should_fail_when_uncompressed_not_allowed() {
		let data = generate_test_data(100);
		let target = TargetCompression {
			compressions: enum_set!(TileCompression::Gzip | TileCompression::Brotli),
			compression_goal: CompressionGoal::UseBestCompression,
		};
		let result = optimize_compression(data, TileCompression::Uncompressed, &target);
		assert!(result.is_err());
	}

	#[test]
	fn should_handle_empty_compression_set_in_recompress() -> Result<()> {
		let original_data = generate_test_data(100);
		let recompressed = recompress(
			original_data.clone(),
			TileCompression::Uncompressed,
			TileCompression::Uncompressed,
		)?;
		assert_eq!(recompressed, original_data);
		Ok(())
	}

	#[test]
	fn test_generic_compress_dispatch() -> Result<()> {
		let data = generate_test_data(1024);
		// Uncompressed should return original data
		let result = compress(data.clone(), TileCompression::Uncompressed)?;
		assert_eq!(result, data);
		// Gzip should match compress_gzip
		let gzip = compress(data.clone(), TileCompression::Gzip)?;
		assert_eq!(gzip, compress_gzip(&data)?);
		// Brotli should match compress_brotli
		let brotli = compress(data.clone(), TileCompression::Brotli)?;
		assert_eq!(brotli, compress_brotli(&data)?);
		Ok(())
	}

	#[test]
	fn test_generic_decompress_dispatch() -> Result<()> {
		let data = generate_test_data(512);
		let gzip = compress_gzip(&data)?;
		let brotli = compress_brotli(&data)?;
		// Uncompressed decompress returns original
		let res_u = decompress(data.clone(), TileCompression::Uncompressed)?;
		assert_eq!(res_u, data);
		// Gzip decompress matches decompress_gzip
		let res_g = decompress(gzip.clone(), TileCompression::Gzip)?;
		assert_eq!(res_g, decompress_gzip(&gzip)?);
		// Brotli decompress matches decompress_brotli
		let res_b = decompress(brotli.clone(), TileCompression::Brotli)?;
		assert_eq!(res_b, decompress_brotli(&brotli)?);
		Ok(())
	}

	#[test]
	fn test_optimize_compression_decompress_when_only_uncompressed_allowed() -> Result<()> {
		let original = generate_test_data(256);
		let gzip_blob = compress_gzip(&original)?;
		let target = TargetCompression::from_none(); // only Uncompressed allowed
		let (out_blob, out_comp) = optimize_compression(gzip_blob.clone(), TileCompression::Gzip, &target)?;
		assert_eq!(out_comp, TileCompression::Uncompressed);
		assert_eq!(out_blob, original);
		// Brotli case
		let brotli_blob = compress_brotli(&original)?;
		let (out_blob2, out_comp2) = optimize_compression(brotli_blob.clone(), TileCompression::Brotli, &target)?;
		assert_eq!(out_comp2, TileCompression::Uncompressed);
		assert_eq!(out_blob2, original);
		Ok(())
	}
}
