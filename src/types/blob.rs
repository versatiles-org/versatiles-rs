//! This module provides the `Blob` struct, a wrapper around `Vec<u8>` that provides additional methods
//! for working with byte data.
//!
//! # Overview
//!
//! The `Blob` struct is a simple wrapper around a `Vec<u8>` that provides methods for creating, accessing,
//! and manipulating byte data. It includes various utility methods for common operations on byte slices,
//! such as creating slices, reading ranges, and converting to and from different types.
//!
//! # Examples
//!
//! ```rust
//! use versatiles::types::Blob;
//!
//! // Creating a new Blob from a vector of bytes
//! let vec = vec![0, 1, 2, 3, 4, 5, 6, 7];
//! let blob = Blob::from(&vec);
//! assert_eq!(blob.len(), 8);
//! assert_eq!(blob.get_range(2..5), &vec![2, 3, 4]);
//! assert_eq!(blob.into_vec(), vec);
//!
//! // Creating a new Blob from a string
//! let text = String::from("Xylofön");
//! let blob = Blob::from(&text);
//! assert_eq!(blob.as_str(), "Xylofön");
//! ```

use anyhow::{bail, Result};
use bytes::Bytes;
use std::fmt::Debug;
use std::ops::Range;

use super::ByteRange;

/// A simple wrapper around `Vec<u8>` that provides additional methods for working with byte data.
#[derive(Clone, PartialEq, Eq)]
pub struct Blob(Vec<u8>);

#[allow(dead_code)]
impl Blob {
	/// Creates an empty `Blob`.
	pub fn new_empty() -> Blob {
		Blob(Vec::new())
	}

	/// Creates a `Blob` with the specified size, filled with zeros.
	///
	/// # Arguments
	///
	/// * `length` - The size of the `Blob`.
	///
	/// # Returns
	///
	/// A `Blob` filled with zeros of the specified size.
	pub fn new_sized(length: usize) -> Blob {
		Blob(vec![0u8; length])
	}

	/// Returns a new `Blob` containing the bytes in the specified range.
	///
	/// # Arguments
	///
	/// * `range` - The range of bytes to extract.
	///
	/// # Returns
	///
	/// A slice of the `Blob` in the specified range.
	pub fn get_range(&self, range: Range<usize>) -> &[u8] {
		&self.0[range]
	}

	/// Returns a new `Blob` containing the bytes in the specified range.
	///
	/// # Arguments
	///
	/// * `range` - The range of bytes to extract as a `ByteRange`.
	///
	/// # Returns
	///
	/// A new `Blob` containing the bytes in the specified range.
	///
	/// # Errors
	///
	/// Returns an error if the range is out of bounds.
	pub fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		if range.offset + range.length > self.0.len() as u64 {
			bail!("read outside range")
		}
		Ok(Blob::from(&self.0[range.as_range_usize()]))
	}

	/// Returns a reference to the underlying byte slice.
	pub fn as_slice(&self) -> &[u8] {
		self.0.as_ref()
	}

	/// Returns a mutable reference to the underlying byte slice.
	pub fn as_mut_slice(&mut self) -> &mut [u8] {
		self.0.as_mut()
	}

	/// Returns a new `Vec<u8>` containing a copy of the underlying bytes.
	pub fn into_vec(self) -> Vec<u8> {
		self.0
	}

	/// Returns the underlying bytes as a string, assuming they represent valid UTF-8 encoded text.
	///
	/// # Panics
	///
	/// Panics if the bytes are not valid UTF-8.
	pub fn as_str(&self) -> &str {
		std::str::from_utf8(&self.0).unwrap()
	}

	/// Converts the `Blob` into a `String`, assuming it contains valid UTF-8 encoded text.
	///
	/// # Panics
	///
	/// Panics if the bytes are not valid UTF-8.
	pub fn into_string(self) -> String {
		String::from_utf8(self.0).unwrap()
	}

	/// Returns a hexadecimal string representation of the underlying bytes.
	pub fn as_hex(&self) -> String {
		self
			.0
			.iter()
			.map(|c| format!("{:02x}", c))
			.collect::<Vec<String>>()
			.join(" ")
	}

	/// Returns the length of the underlying byte slice.
	pub fn len(&self) -> u64 {
		self.0.len() as u64
	}

	/// Returns `true` if the underlying byte slice is empty, `false` otherwise.
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

impl From<Bytes> for Blob {
	/// Converts a `bytes::Bytes` instance into a `Blob`.
	fn from(item: Bytes) -> Self {
		Blob(item.to_vec())
	}
}

impl From<Vec<u8>> for Blob {
	/// Converts a `Vec<u8>` instance into a `Blob`.
	fn from(item: Vec<u8>) -> Self {
		Blob(item)
	}
}

impl From<&Vec<u8>> for Blob {
	/// Converts a `Vec<u8>` instance into a `Blob`.
	fn from(item: &Vec<u8>) -> Self {
		Blob(item.clone())
	}
}

impl From<&[u8]> for Blob {
	/// Converts a slice into a `Blob`.
	fn from(item: &[u8]) -> Self {
		Blob(item.to_vec())
	}
}

impl From<&str> for Blob {
	/// Converts a `&str` instance into a `Blob`.
	fn from(item: &str) -> Self {
		Blob(item.as_bytes().to_vec())
	}
}

impl From<&String> for Blob {
	/// Converts a `&String` instance into a `Blob`.
	fn from(item: &String) -> Self {
		Blob(item.as_bytes().to_vec())
	}
}

impl From<String> for Blob {
	/// Converts a `String` instance into a `Blob`.
	fn from(item: String) -> Self {
		Blob(item.into_bytes())
	}
}

impl Debug for Blob {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_fmt(format_args!("Blob({}): {}", self.0.len(), self.as_hex()))
	}
}

impl std::fmt::Display for Blob {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", String::from_utf8_lossy(&self.0))
	}
}

impl Default for Blob {
	fn default() -> Self {
		Self::new_empty()
	}
}

unsafe impl Send for Blob {}
unsafe impl Sync for Blob {}

#[cfg(test)]
mod tests {
	use bytes::Bytes;

	// Import the Blob struct from the parent module
	use super::Blob;

	// Test basic functionality of the Blob struct
	#[test]
	fn basic_tests() {
		// Create a vector of bytes
		let vec = vec![0, 1, 2, 3, 4, 5, 6, 7];

		// Create a Blob instance from the vector
		let blob = Blob::from(&vec);

		// Assert that the Blob's length is correct
		assert_eq!(blob.len(), 8);

		// Assert that a range of bytes can be extracted from the Blob correctly
		assert_eq!(blob.get_range(2..5), &vec![2, 3, 4]);

		// Assert that the Blob's underlying bytes are the same as the original vector
		assert_eq!(blob.into_vec(), vec);
	}

	// Test creating a Blob from a string
	#[test]
	fn string() {
		// Create a string with non-ASCII characters
		let text = String::from("Xylofön");

		// Assert that a Blob can be created from the string and converted back to a string correctly
		assert_eq!(Blob::from(text.clone()).as_str(), text);

		// Assert that a Blob can be created from a reference to the string and converted back to a string correctly
		assert_eq!(Blob::from(&text).as_str(), text);
		assert_eq!(Blob::from(&text).to_string(), text);

		// Assert that a Blob can be created from a reference to the string's underlying buffer and converted back to a string correctly
		assert_eq!(Blob::from(&*text).as_str(), text);
	}

	// Test creating an empty Blob
	#[test]
	fn empty() {
		// Create an empty string
		let text = String::from("");

		// Assert that a Blob can be created from the empty string and correctly identified as empty
		assert!(Blob::from(&text).is_empty());
	}

	// Test creating a Blob from bytes
	#[test]
	fn bytes() {
		// Create a string with non-ASCII characters
		let text = String::from("Smørrebrød");

		let bytes = Bytes::from(text.clone());

		// Assert that a Blob can be created from the Bytes instance and converted back to the string correctly
		assert_eq!(Blob::from(bytes).as_str(), text);
	}

	// Test the debug format of the Blob struct
	#[test]
	fn debug() {
		assert_eq!(
			format!("{:?}", Blob::from("Voisilmäpulla")),
			"Blob(14): 56 6f 69 73 69 6c 6d c3 a4 70 75 6c 6c 61"
		);
		assert_eq!(
			format!("{:?}", Blob::from("01234567890123456789012345678901")),
			"Blob(32): 30 31 32 33 34 35 36 37 38 39 30 31 32 33 34 35 36 37 38 39 30 31 32 33 34 35 36 37 38 39 30 31"
		);
	}
}
