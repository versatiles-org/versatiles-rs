//! Module for representing and manipulating ranges of block sizes for tile traversal.
//!
//! A `TraversalSize` encodes a range of block sizes as powers of two (log2 values),
//! controlling how many tiles are grouped per traversal block.

use anyhow::{Result, ensure};
use versatiles_derive::context;

/// Represents allowed sizes of a block of tiles
/// The size is represented as log2(size).
/// For example, TraversalSize { max: 6 } represents block of sizes up to 64 tiles.
#[derive(Clone, PartialEq)]
pub struct TraversalSize {
	min: u8,
	max: u8,
}

impl TraversalSize {
	/// Create a new `TraversalSize` covering sizes from `min_size` up to `max_size`.
	///
	/// Both `min_size` and `max_size` must be positive powers of two, and `min_size <= max_size`.
	///
	/// # Errors
	/// Returns an error if sizes are zero, not powers of two, out of order, or too large.
	#[context("Failed to create TraversalSize")]
	pub fn new(min_size: u32, max_size: u32) -> Result<TraversalSize> {
		ensure!(min_size <= max_size, "min size must be less than or equal to max size");
		Ok(TraversalSize {
			min: size_to_bits(min_size)?,
			max: size_to_bits(max_size)?,
		})
	}

	/// Return a default `TraversalSize` covering the full range of valid sizes (1 to 2^31).
	pub const fn new_default() -> Self {
		TraversalSize { min: 0, max: 20 }
	}

	/// Shortcut to create a `TraversalSize` with minimum size 1 and maximum size `size`.
	pub fn new_max(size: u32) -> Result<TraversalSize> {
		TraversalSize::new(1, size)
	}

	/// Check whether the size range is empty (min > max).
	pub fn is_empty(&self) -> bool {
		self.min > self.max
	}

	/// Return the maximum allowed block size.
	///
	/// # Errors
	/// Returns an error if the range is empty or `max` is out of bounds.
	pub fn max_size(&self) -> Result<u32> {
		ensure!(!self.is_empty(), "TraversalSize is empty: {self:?}");
		ensure!(self.max <= 20, "TraversalSize max is too large: {self:?}");
		Ok(1 << self.max)
	}

	/// Return the minimum allowed block size.
	///
	/// # Errors
	/// Returns an error if the range is empty.
	pub fn min_size(&self) -> Result<u32> {
		ensure!(!self.is_empty(), "TraversalSize is empty: {self:?}");
		Ok(1 << self.min)
	}

	/// Restrict this range to the intersection with another `TraversalSize`.
	///
	/// # Errors
	/// Returns an error if the resulting range is empty (no overlap).
	pub fn intersect(&mut self, other: &TraversalSize) -> Result<()> {
		let min = self.min.max(other.min);
		let max = self.max.min(other.max);
		ensure!(min <= max, "Non-overlapping traversal sizes: {self:?} and {other:?}");
		self.min = min;
		self.max = max;
		Ok(())
	}

	pub fn get_intersected(&self, other: &TraversalSize) -> Result<TraversalSize> {
		let mut result = self.clone();
		result.intersect(other)?;
		Ok(result)
	}
}

impl Default for TraversalSize {
	fn default() -> Self {
		TraversalSize::new_default()
	}
}

impl std::fmt::Debug for TraversalSize {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		if self.is_empty() {
			write!(f, "TraversalSize (empty)")
		} else {
			write!(f, "TraversalSize ({}..{})", 1 << self.min, 1 << self.max)
		}
	}
}

/// Convert a block size (power of two) into its log2 (bit) representation.
///
/// # Errors
/// Returns an error if `size` is zero, not a power of two, or too large.
fn size_to_bits(size: u32) -> Result<u8> {
	ensure!(size > 0, "Size must be greater than zero");
	ensure!(size.is_power_of_two(), "Size must be a power of two, but is {size}");
	let bits = (size as f64).log2().floor() as u8;
	ensure!(bits < 32, "Size {size} is too large");
	Ok(bits)
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;

	fn extract_error_lines<T: std::fmt::Debug>(result: anyhow::Result<T>) -> Vec<String> {
		result.unwrap_err().chain().map(|e| e.to_string()).collect::<Vec<_>>()
	}

	#[test]
	fn test_new_zero_min_errors() {
		assert_eq!(
			extract_error_lines(TraversalSize::new(0, 1)),
			["Failed to create TraversalSize", "Size must be greater than zero"]
		);
	}

	#[test]
	fn test_new_valid_and_get_max_size() -> Result<()> {
		let ts = TraversalSize::new(2, 8)?;
		assert_eq!(ts.max_size()?, 8);
		Ok(())
	}

	#[test]
	fn test_new_max() -> Result<()> {
		let ts = TraversalSize::new_max(16)?;
		assert_eq!(ts.max_size()?, 16);
		Ok(())
	}

	#[test]
	fn test_intersect_overlapping() -> Result<()> {
		let mut ts1 = TraversalSize::new(2, 16)?;
		let ts2 = TraversalSize::new(4, 32)?;
		ts1.intersect(&ts2)?;
		assert_eq!(ts1.max_size()?, 16);
		assert_eq!(format!("{ts1:?}"), "TraversalSize (4..16)");
		Ok(())
	}

	#[test]
	fn test_intersect_non_overlapping_errors_and_empty_state() -> Result<()> {
		let mut ts1 = TraversalSize::new(2, 4)?;
		let ts2 = TraversalSize::new(8, 16)?;
		assert_eq!(
			extract_error_lines(ts1.intersect(&ts2)),
			["Non-overlapping traversal sizes: TraversalSize (2..4) and TraversalSize (8..16)"]
		);
		Ok(())
	}

	#[test]
	fn test_default_and_debug() -> Result<()> {
		let ts = TraversalSize::default();
		assert_eq!(ts.max_size()?, 1 << 20);
		assert_eq!(format!("{ts:?}"), "TraversalSize (1..1048576)");
		Ok(())
	}

	#[test]
	fn test_min_greater_than_max_error() {
		assert_eq!(
			extract_error_lines(TraversalSize::new(16, 8)),
			[
				"Failed to create TraversalSize",
				"min size must be less than or equal to max size"
			]
		);
	}

	#[test]
	fn test_non_power_of_two_error() {
		assert_eq!(
			extract_error_lines(TraversalSize::new(3, 8)),
			[
				"Failed to create TraversalSize",
				"Size must be a power of two, but is 3"
			]
		);
	}

	#[test]
	fn test_small_sizes() -> Result<()> {
		let ts = TraversalSize::new(1, 2)?;
		assert_eq!(ts.max_size()?, 2);
		assert!(!ts.is_empty());
		Ok(())
	}

	#[test]
	fn test_intersect_partial_overlap() -> Result<()> {
		let mut ts1 = TraversalSize::new(8, 64)?;
		let ts2 = TraversalSize::new(2, 32)?;
		ts1.intersect(&ts2)?;
		assert_eq!(ts1.max_size()?, 32);
		assert_eq!(format!("{ts1:?}"), "TraversalSize (8..32)");
		Ok(())
	}
}
