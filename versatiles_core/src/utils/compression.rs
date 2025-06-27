//! # Compression Module
//!
//! This module provides functionalities to compress and decompress data blobs
//! using various compression algorithms such as Gzip and Brotli. It also allows
//! optimizing compression based on target preferences and handling recompression.
//!
//! ## Features
//! - Compress and decompress data using Gzip and Brotli.
//! - Optimize compression based on target settings.
//! - Recompress data from one compression format to another.
//!
//! ## Usage
//! ```rust
//! use versatiles_core::{utils::*, types::*};
//!
//! let data = Blob::from(vec![1, 2, 3, 4, 5]);
//! let compressed = compress_gzip(&data)?;
//! let decompressed = decompress_gzip(&compressed)?;
//! assert_eq!(data, decompressed);
//! # Ok::<(), anyhow::Error>(())
//! ```

#![allow(dead_code)]

use crate::types::{Blob, TileCompression};
use anyhow::{bail, Context, Result};
use brotli::{enc::BrotliEncoderParams, BrotliCompress, BrotliDecompress};
use enumset::EnumSet;
use flate2::bufread::{GzDecoder, GzEncoder};
use std::{
	fmt::{self, Debug},
	io::{Cursor, Read},
};

/// Represents the target compression settings.
#[derive(PartialEq)]
pub struct TargetCompression {
	/// Set of allowed compression algorithms.
	compressions: EnumSet<TileCompression>,
	/// Desired compression goal.
	compression_goal: CompressionGoal,
}

/// Defines the desired compression objective.
#[derive(Clone, Copy, PartialEq)]
pub enum CompressionGoal {
	/// Prioritize speed over compression ratio.
	UseFastCompression,
	/// Prioritize compression ratio over speed.
	UseBestCompression,
	/// Treat data as incompressible.
	IsIncompressible,
}

impl Debug for CompressionGoal {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::UseFastCompression => write!(f, "Use Fast Compression"),
			Self::UseBestCompression => write!(f, "Use Best Compression"),
			Self::IsIncompressible => write!(f, "Is Incompressible"),
		}
	}
}

impl TargetCompression {
	/// Creates a new `TargetCompression` with a set of allowed compressions.
	///
	/// By default, the compression goal is set to `UseBestCompression`.
	///
	/// # Arguments
	///
	/// * `compressions` - A set of allowed compression algorithms.
	///
	/// # Returns
	///
	/// * `TargetCompression` instance.
	pub fn from_set(compressions: EnumSet<TileCompression>) -> Self {
		TargetCompression {
			compressions,
			compression_goal: CompressionGoal::UseBestCompression,
		}
	}

	/// Creates a new `TargetCompression` allowing only the specified compression.
	///
	/// The compression goal is set to `UseBestCompression`.
	///
	/// # Arguments
	///
	/// * `compression` - A single compression algorithm to allow.
	///
	/// # Returns
	///
	/// * `TargetCompression` instance.
	pub fn from(compression: TileCompression) -> Self {
		Self::from_set(EnumSet::only(compression))
	}

	/// Creates a new `TargetCompression` allowing no compression.
	///
	/// The compression goal is set to `UseBestCompression`, but since no compression is allowed,
	/// data will remain uncompressed.
	///
	/// # Returns
	///
	/// * `TargetCompression` instance.
	pub fn from_none() -> Self {
		Self::from(TileCompression::Uncompressed)
	}

	/// Sets the compression goal to prioritize speed.
	pub fn set_fast_compression(&mut self) {
		self.compression_goal = CompressionGoal::UseFastCompression;
	}

	/// Sets the compression goal to treat data as incompressible.
	pub fn set_incompressible(&mut self) {
		self.compression_goal = CompressionGoal::IsIncompressible;
	}

	/// Checks if a specific compression algorithm is allowed.
	///
	/// # Arguments
	///
	/// * `compression` - The compression algorithm to check.
	///
	/// # Returns
	///
	/// * `true` if the compression is allowed.
	/// * `false` otherwise.
	pub fn contains(&self, compression: TileCompression) -> bool {
		self.compressions.contains(compression)
	}

	/// Adds a compression algorithm to the allowed set.
	///
	/// # Arguments
	///
	/// * `compression` - The compression algorithm to add.
	pub fn insert(&mut self, compression: TileCompression) {
		self.compressions.insert(compression);
	}
}

impl Debug for TargetCompression {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("TargetCompression")
			.field("allowed_compressions", &self.compressions)
			.field("compression_goal", &self.compression_goal)
			.finish()
	}
}

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
pub fn optimize_compression(
	blob: Blob,
	input_compression: &TileCompression,
	target: &TargetCompression,
) -> Result<(Blob, TileCompression)> {
	if target.compressions.is_empty() {
		bail!("At least one compression algorithm must be allowed");
	}

	if !target.compressions.contains(TileCompression::Uncompressed) {
		bail!("'Uncompressed' must always be supported");
	}

	use CompressionGoal::*;

	// If the target is not seeking the best compression and the current compression is allowed,
	// retain the current compression.
	if target.compression_goal != UseBestCompression && target.compressions.contains(*input_compression) {
		return Ok((blob, *input_compression));
	}

	match input_compression {
		TileCompression::Uncompressed => {
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
			if target.compression_goal != IsIncompressible && target.compressions.contains(TileCompression::Brotli) {
				let decompressed = decompress_gzip(&blob).context("Failed to decompress Gzip blob")?;
				let compressed_brotli = compress_brotli(&decompressed).context("Failed to compress Brotli blob")?;
				return Ok((compressed_brotli, TileCompression::Brotli));
			}

			if target.compressions.contains(TileCompression::Gzip) {
				return Ok((blob, TileCompression::Gzip));
			}

			// Fallback to Uncompressed if Gzip is not allowed
			let decompressed = decompress_gzip(&blob).context("Failed to decompress Gzip blob")?;
			Ok((decompressed, TileCompression::Uncompressed))
		}
		TileCompression::Brotli => {
			if target.compressions.contains(TileCompression::Brotli) {
				return Ok((blob, TileCompression::Brotli));
			}
			let decompressed = decompress_brotli(&blob).context("Failed to decompress Brotli blob")?;

			if target.compression_goal != IsIncompressible && target.compressions.contains(TileCompression::Gzip) {
				let compressed_gzip = compress_gzip(&decompressed).context("Failed to compress Gzip blob")?;
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
pub fn recompress(
	blob: Blob,
	input_compression: &TileCompression,
	output_compression: &TileCompression,
) -> Result<Blob> {
	if input_compression == output_compression {
		return Ok(blob);
	}
	let decompressed = decompress(blob, input_compression)
		.with_context(|| format!("Failed to decompress using {input_compression:?}"))?;
	let recompressed = compress(decompressed, output_compression)
		.with_context(|| format!("Failed to compress using {output_compression:?}"))?;
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
pub fn compress(blob: Blob, compression: &TileCompression) -> Result<Blob> {
	match compression {
		TileCompression::Uncompressed => Ok(blob),
		TileCompression::Gzip => compress_gzip(&blob),
		TileCompression::Brotli => compress_brotli(&blob),
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
pub fn decompress(blob: Blob, compression: &TileCompression) -> Result<Blob> {
	match compression {
		TileCompression::Uncompressed => Ok(blob),
		TileCompression::Gzip => decompress_gzip(&blob),
		TileCompression::Brotli => decompress_brotli(&blob),
	}
}

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
pub fn decompress_gzip(blob: &Blob) -> Result<Blob> {
	let mut decoder = GzDecoder::new(blob.as_slice());
	let mut decompressed_data = Vec::new();
	decoder
		.read_to_end(&mut decompressed_data)
		.context("Failed to decompress data using Gzip")?;
	Ok(Blob::from(decompressed_data))
}

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
	use super::*;
	use enumset::enum_set;

	/// Generates deterministic pseudo-random binary data of a specified size.
	///
	/// # Arguments
	///
	/// * `size` - The size of the data to generate.
	///
	/// # Returns
	///
	/// * `Blob` containing the generated data.
	fn generate_test_data(size: usize) -> Blob {
		let mut data = Vec::with_capacity(size);
		for i in 0..size {
			data.push((((i as f64 + 1.0).cos() * 1_000_000.0) as u8).wrapping_add(i as u8));
		}
		Blob::from(data)
	}

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

	#[test]
	fn should_compress_and_decompress_gzip_correctly() -> Result<()> {
		let data = generate_test_data(100_000);
		let compressed = compress_gzip(&data)?;
		let decompressed = decompress_gzip(&compressed)?;
		assert_eq!(data, decompressed, "Gzip compression and decompression failed");
		Ok(())
	}

	#[test]
	/// Tests the `optimize_compression` function across various compression scenarios.
	fn should_optimize_compression_correctly() -> Result<()> {
		let original_blob = generate_test_data(100);
		let gzip_blob = compress_gzip(&original_blob)?;
		let brotli_blob = compress_brotli(&original_blob)?;

		let test_case = |input_compression: TileCompression,
		                 allowed_compressions: EnumSet<TileCompression>,
		                 goal: CompressionGoal,
		                 expected_compression: TileCompression|
		 -> Result<()> {
			let target = TargetCompression {
				compressions: allowed_compressions,
				compression_goal: goal,
			};
			let input_blob = match input_compression {
				TileCompression::Uncompressed => original_blob.clone(),
				TileCompression::Gzip => gzip_blob.clone(),
				TileCompression::Brotli => brotli_blob.clone(),
			};
			let expected_blob = match expected_compression {
				TileCompression::Uncompressed => original_blob.clone(),
				TileCompression::Gzip => gzip_blob.clone(),
				TileCompression::Brotli => brotli_blob.clone(),
			};
			let (result_blob, result_compression) = optimize_compression(input_blob, &input_compression, &target)?;
			assert_eq!(
				result_compression, expected_compression,
				"Expected compression {expected_compression:?}, but got {result_compression:?}"
			);
			assert_eq!(
				result_blob, expected_blob,
				"Expected blob data does not match the result blob"
			);
			Ok(())
		};

		let uncompressed = TileCompression::Uncompressed;
		let gzip = TileCompression::Gzip;
		let brotli = TileCompression::Brotli;

		let allowed_uncompressed = enum_set!(TileCompression::Uncompressed);
		let allowed_gzip = enum_set!(TileCompression::Uncompressed | TileCompression::Gzip);
		let allowed_brotli = enum_set!(TileCompression::Uncompressed | TileCompression::Brotli);
		let allowed_all = enum_set!(TileCompression::Uncompressed | TileCompression::Gzip | TileCompression::Brotli);

		use CompressionGoal::*;

		// Test using best compression
		test_case(uncompressed, allowed_all, UseBestCompression, brotli)?;
		test_case(gzip, allowed_all, UseBestCompression, brotli)?;
		test_case(brotli, allowed_all, UseBestCompression, brotli)?;

		// Test using fast compression
		test_case(uncompressed, allowed_all, UseFastCompression, uncompressed)?;
		test_case(gzip, allowed_gzip, UseFastCompression, gzip)?;
		test_case(gzip, allowed_brotli, UseFastCompression, brotli)?;
		test_case(brotli, allowed_all, UseFastCompression, brotli)?;

		// Test treating data as incompressible
		test_case(uncompressed, allowed_uncompressed, IsIncompressible, uncompressed)?;
		test_case(gzip, allowed_gzip, IsIncompressible, gzip)?;
		test_case(brotli, allowed_brotli, IsIncompressible, brotli)?;

		Ok(())
	}

	#[test]
	fn should_recompress_correctly() -> Result<()> {
		let original_data = generate_test_data(1_000);
		let gzip_data = compress_gzip(&original_data)?;
		let brotli_data = compress_brotli(&original_data)?;

		// Recompress Gzip to Brotli
		let recompressed = recompress(gzip_data.clone(), &TileCompression::Gzip, &TileCompression::Brotli)?;
		let decompressed = decompress_brotli(&recompressed)?;
		assert_eq!(original_data, decompressed, "Recompression from Gzip to Brotli failed");

		// Recompress Brotli to Gzip
		let recompressed = recompress(brotli_data.clone(), &TileCompression::Brotli, &TileCompression::Gzip)?;
		let decompressed = decompress_gzip(&recompressed)?;
		assert_eq!(original_data, decompressed, "Recompression from Brotli to Gzip failed");

		// Recompress Gzip to Gzip (no change)
		let recompressed = recompress(gzip_data.clone(), &TileCompression::Gzip, &TileCompression::Gzip)?;
		assert_eq!(
			recompressed, gzip_data,
			"Recompression from Gzip to Gzip should not alter data"
		);

		// Recompress Uncompressed to Gzip
		let recompressed = recompress(
			original_data.clone(),
			&TileCompression::Uncompressed,
			&TileCompression::Gzip,
		)?;
		let decompressed = decompress_gzip(&recompressed)?;
		assert_eq!(
			original_data, decompressed,
			"Recompression from Uncompressed to Gzip failed"
		);

		Ok(())
	}

	#[test]
	fn should_handle_no_compression_correctly() -> Result<()> {
		let data = generate_test_data(500);
		let result = optimize_compression(
			data.clone(),
			&TileCompression::Uncompressed,
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
		let result = optimize_compression(data, &TileCompression::Uncompressed, &target);
		assert!(result.is_err(), "Expected error when no compressions are allowed");
	}

	#[test]
	fn should_fail_when_uncompressed_not_allowed() {
		let data = generate_test_data(100);
		let target = TargetCompression {
			compressions: enum_set!(TileCompression::Gzip | TileCompression::Brotli),
			compression_goal: CompressionGoal::UseBestCompression,
		};
		let result = optimize_compression(data, &TileCompression::Uncompressed, &target);
		assert!(result.is_err(), "Expected error when 'Uncompressed' is not allowed");
	}

	#[test]
	fn should_handle_empty_compression_set_in_recompress() -> Result<()> {
		let original_data = generate_test_data(100);
		let recompressed = recompress(
			original_data.clone(),
			&TileCompression::Uncompressed,
			&TileCompression::Uncompressed,
		)?;
		assert_eq!(
			recompressed, original_data,
			"Recompressing Uncompressed to Uncompressed should not alter data"
		);
		Ok(())
	}
}
