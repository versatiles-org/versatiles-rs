#![allow(dead_code)]

use std::fmt;
use std::ops::Range;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ByteRange {
	pub offset: u64,
	pub length: u64,
}

impl ByteRange {
	pub fn new(offset: u64, length: u64) -> Self {
		Self { offset, length }
	}

	pub fn empty() -> Self {
		Self { offset: 0, length: 0 }
	}

	pub fn get_shifted_forward(&self, offset: u64) -> Self {
		Self {
			offset: self.offset + offset,
			length: self.length,
		}
	}

	pub fn get_shifted_backward(&self, offset: u64) -> Self {
		Self {
			offset: self.offset - offset,
			length: self.length,
		}
	}

	pub fn shift_forward(&mut self, offset: u64) {
		self.offset += offset;
	}

	pub fn shift_backward(&mut self, offset: u64) {
		self.offset -= offset;
	}

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
