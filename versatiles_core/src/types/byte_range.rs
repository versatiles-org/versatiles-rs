//! This module provides the `ByteRange` struct, which represents a range of bytes with an offset and length.
//!
//! # Overview
//!
//! The `ByteRange` struct is used to represent a contiguous range of bytes with a specified offset and length.
//! It provides various utility methods for creating, manipulating, and converting byte ranges.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::types::ByteRange;
//!
//! let range = ByteRange::new(23, 42);
//! assert_eq!(range.offset, 23);
//! assert_eq!(range.length, 42);
//! assert_eq!(range.as_range_usize().start, 23);
//! assert_eq!(range.as_range_usize().end, 65); // 23 + 42 = 65
//! ```

use std::fmt;
use std::ops::Range;

/// A struct representing a range of bytes with an offset and length.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ByteRange {
	/// The starting offset of the byte range.
	pub offset: u64,
	/// The length of the byte range.
	pub length: u64,
}

#[allow(dead_code)]
impl ByteRange {
	/// Creates a new `ByteRange` with the specified offset and length.
	///
	/// # Arguments
	///
	/// * `offset` - The starting offset of the byte range.
	/// * `length` - The length of the byte range.
	///
	/// # Returns
	///
	/// A new `ByteRange` instance.
	pub fn new(offset: u64, length: u64) -> Self {
		Self { offset, length }
	}

	/// Creates an empty `ByteRange` with zero offset and length.
	///
	/// # Returns
	///
	/// An empty `ByteRange` instance.
	pub fn empty() -> Self {
		Self {
			offset: 0,
			length: 0,
		}
	}

	/// Returns a new `ByteRange` that is shifted forward by the specified offset.
	///
	/// # Arguments
	///
	/// * `offset` - The amount to shift the range forward.
	///
	/// # Returns
	///
	/// A new `ByteRange` instance shifted forward by the specified offset.
	pub fn get_shifted_forward(&self, offset: u64) -> Self {
		Self {
			offset: self.offset + offset,
			length: self.length,
		}
	}

	/// Returns a new `ByteRange` that is shifted backward by the specified offset.
	///
	/// # Arguments
	///
	/// * `offset` - The amount to shift the range backward.
	///
	/// # Returns
	///
	/// A new `ByteRange` instance shifted backward by the specified offset.
	pub fn get_shifted_backward(&self, offset: u64) -> Self {
		Self {
			offset: self.offset - offset,
			length: self.length,
		}
	}

	/// Shifts the current `ByteRange` forward by the specified offset.
	///
	/// # Arguments
	///
	/// * `offset` - The amount to shift the range forward.
	pub fn shift_forward(&mut self, offset: u64) {
		self.offset += offset;
	}

	/// Shifts the current `ByteRange` backward by the specified offset.
	///
	/// # Arguments
	///
	/// * `offset` - The amount to shift the range backward.
	pub fn shift_backward(&mut self, offset: u64) {
		self.offset -= offset;
	}

	/// Converts the `ByteRange` to a `Range<usize>`.
	///
	/// # Returns
	///
	/// A `Range<usize>` representing the byte range.
	pub fn as_range_usize(&self) -> Range<usize> {
		Range {
			start: self.offset as usize,
			end: (self.offset + self.length) as usize,
		}
	}
}

impl fmt::Debug for ByteRange {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "ByteRange[{},{}]", self.offset, self.length)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn new() {
		let range = ByteRange::new(23, 42);
		assert_eq!(range.offset, 23);
		assert_eq!(range.length, 42);
	}

	#[test]
	fn empty() {
		let range = ByteRange::empty();
		assert_eq!(range.offset, 0);
		assert_eq!(range.length, 0);
	}

	#[test]
	fn as_range_usize() {
		let range = ByteRange::new(23, 42);
		let range_usize = range.as_range_usize();
		assert_eq!(range_usize.start, 23);
		assert_eq!(range_usize.end, 65); // 23 + 42 = 65
	}

	#[test]
	fn debug() {
		let range = ByteRange::new(23, 42);
		assert_eq!(format!("{:?}", range), "ByteRange[23,42]");
	}
}
