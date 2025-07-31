//! This module provides the [`ByteRange`] struct, which represents a range of bytes
//! by offset and length.
//!
//! # Overview
//!
//! The [`ByteRange`] struct is used to represent a contiguous range of bytes with a specified offset
//! and length. It provides various utility methods for creating, manipulating, and converting
//! byte ranges.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::ByteRange;
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
///
/// `ByteRange` is primarily used to indicate which subsection of a larger data buffer
/// should be read, written, or otherwise processed. The offset indicates the starting
/// position, and the length determines how many bytes are included.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ByteRange {
	/// The starting offset of the byte range.
	pub offset: u64,
	/// The length of the byte range in bytes.
	pub length: u64,
}

impl ByteRange {
	/// Creates a new `ByteRange` with the specified `offset` and `length`.
	///
	/// # Arguments
	///
	/// * `offset` - The starting offset of the byte range.
	/// * `length` - The number of bytes in the range.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::ByteRange;
	///
	/// let range = ByteRange::new(10, 5);
	/// assert_eq!(range.offset, 10);
	/// assert_eq!(range.length, 5);
	/// ```
	pub fn new(offset: u64, length: u64) -> Self {
		Self { offset, length }
	}

	/// Creates an empty `ByteRange`, starting at offset 0 with length 0.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::ByteRange;
	///
	/// let empty = ByteRange::empty();
	/// assert_eq!(empty.offset, 0);
	/// assert_eq!(empty.length, 0);
	/// ```
	pub fn empty() -> Self {
		Self { offset: 0, length: 0 }
	}

	/// Returns a new `ByteRange` that is shifted forward by the specified `offset`.
	///
	/// This method does not mutate the original `ByteRange`.
	///
	/// # Arguments
	///
	/// * `offset` - The number of bytes to shift the range forward.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::ByteRange;
	///
	/// let r1 = ByteRange::new(10, 5);
	/// let r2 = r1.get_shifted_forward(7);
	/// assert_eq!(r2.offset, 17);
	/// assert_eq!(r2.length, 5);
	/// assert_eq!(r1.offset, 10); // original remains unchanged
	/// ```
	pub fn get_shifted_forward(&self, offset: u64) -> Self {
		Self {
			offset: self.offset + offset,
			length: self.length,
		}
	}

	/// Returns a new `ByteRange` that is shifted backward by the specified `offset`.
	///
	/// This method does not mutate the original `ByteRange`.  
	/// **Note:** It is the caller's responsibility to ensure that `self.offset >= offset`;  
	/// otherwise, the resulting offset could be negative when interpreted as `u64`.
	///
	/// # Arguments
	///
	/// * `offset` - The number of bytes to shift the range backward.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::ByteRange;
	///
	/// let r1 = ByteRange::new(10, 5);
	/// let r2 = r1.get_shifted_backward(3);
	/// assert_eq!(r2.offset, 7);
	/// assert_eq!(r2.length, 5);
	/// ```
	pub fn get_shifted_backward(&self, offset: u64) -> Self {
		Self {
			offset: self.offset - offset,
			length: self.length,
		}
	}

	/// Shifts the current `ByteRange` forward (in-place) by the specified `offset`.
	///
	/// # Arguments
	///
	/// * `offset` - The number of bytes to shift the range forward.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::ByteRange;
	///
	/// let mut range = ByteRange::new(10, 5);
	/// range.shift_forward(3);
	/// assert_eq!(range.offset, 13);
	/// ```
	pub fn shift_forward(&mut self, offset: u64) {
		self.offset = self.offset.wrapping_add(offset);
	}

	/// Shifts the current `ByteRange` backward (in-place) by the specified `offset`.
	///
	/// **Note:** If `offset` is larger than `self.offset`, this can cause wrapping behavior
	/// in an unchecked environment. For safety, ensure that `self.offset >= offset`
	/// before calling this method.
	///
	/// # Arguments
	///
	/// * `offset` - The number of bytes to shift the range backward.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::ByteRange;
	///
	/// let mut range = ByteRange::new(10, 5);
	/// range.shift_backward(5);
	/// assert_eq!(range.offset, 5);
	/// ```
	pub fn shift_backward(&mut self, offset: u64) {
		self.offset = self.offset.wrapping_sub(offset);
	}

	/// Converts the `ByteRange` to a `std::ops::Range<usize>`.
	///
	/// # Returns
	///
	/// A `Range<usize>` from `offset` to `offset + length`.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::ByteRange;
	///
	/// let range = ByteRange::new(23, 42);
	/// let usize_range = range.as_range_usize();
	/// assert_eq!(usize_range.start, 23);
	/// assert_eq!(usize_range.end, 65);
	/// ```
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

impl fmt::Display for ByteRange {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "[{}..{}]", self.offset, self.offset + self.length - 1)
	}
}

#[cfg(test)]
mod tests {
	use super::ByteRange;

	/// Verifies that `new` correctly sets offset and length.
	#[test]
	fn test_new() {
		let range = ByteRange::new(23, 42);
		assert_eq!(range.offset, 23, "Expected offset == 23");
		assert_eq!(range.length, 42, "Expected length == 42");
	}

	/// Checks that `empty` creates a ByteRange of offset=0, length=0.
	#[test]
	fn test_empty() {
		let range = ByteRange::empty();
		assert_eq!(range.offset, 0, "Expected offset == 0 for empty");
		assert_eq!(range.length, 0, "Expected length == 0 for empty");
	}

	/// Validates the `as_range_usize` conversion.
	#[test]
	fn test_as_range_usize() {
		let range = ByteRange::new(23, 42);
		let range_usize = range.as_range_usize();
		assert_eq!(range_usize.start, 23, "start should match offset");
		assert_eq!(range_usize.end, 65, "end should be offset + length = 65");
	}

	/// Ensures `get_shifted_forward` does not alter the original range and returns a new range with an increased offset.
	#[test]
	fn test_get_shifted_forward() {
		let original = ByteRange::new(10, 5);
		let shifted = original.get_shifted_forward(3);
		assert_eq!(shifted.offset, 13, "Offset should be 10 + 3 = 13");
		assert_eq!(shifted.length, 5, "Length should remain unchanged");
		// Original should remain unchanged
		assert_eq!(original.offset, 10, "Original offset unchanged");
	}

	/// Ensures `get_shifted_backward` does not alter the original range and returns a new range with a decreased offset.
	#[test]
	fn test_get_shifted_backward() {
		let original = ByteRange::new(10, 5);
		let shifted = original.get_shifted_backward(5);
		assert_eq!(shifted.offset, 5, "Offset should be 10 - 5 = 5");
		assert_eq!(shifted.length, 5, "Length should remain unchanged");
		// Original should remain unchanged
		assert_eq!(original.offset, 10, "Original offset unchanged");
	}

	/// Confirms `shift_forward` mutates the offset in place.
	#[test]
	fn test_shift_forward() {
		let mut range = ByteRange::new(10, 5);
		range.shift_forward(7);
		assert_eq!(range.offset, 17, "Offset should be 10 + 7 = 17");
	}

	/// Confirms `shift_backward` mutates the offset in place.
	#[test]
	fn test_shift_backward() {
		let mut range = ByteRange::new(20, 10);
		range.shift_backward(5);
		assert_eq!(range.offset, 15, "Offset should be 20 - 5 = 15");
	}

	/// Verifies that debug output matches the expected format.
	#[test]
	fn test_debug() {
		let range = ByteRange::new(23, 42);
		assert_eq!(format!("{range:?}"), "ByteRange[23,42]");
	}

	/// Illustrates potential issues if offset is subtracted beyond zero without checks.
	/// By default, Rust will wrap around in debug mode it might panic, but in release mode
	/// it may silently wrap. This test only demonstrates the behavior.
	#[test]
	fn test_underflow_behavior() {
		let mut range = ByteRange::new(2, 5);
		// Shifting backward by more than offset can cause underflow/wrap
		range.shift_backward(5);
		// In release mode, offset may wrap around to a large number due to u64 underflow
		// For demonstration, we just show it's not guaranteed to be safe.
		// No specific assert here, but let's check offset != 2 to confirm it changed.
		assert_ne!(range.offset, 2, "Offset changed unpredictably if shift < offset");
	}
}
