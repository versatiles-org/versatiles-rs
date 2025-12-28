//! This module provides the [`Blob`] struct, a wrapper around [`Vec<u8>`] that provides additional methods
//! for working with byte data.
//!
//! # Overview
//!
//! The [`Blob`] struct is a simple wrapper around a `Vec<u8>` that provides methods for creating, accessing,
//! and manipulating byte data. It includes various utility methods for common operations on byte slices,
//! such as creating slices, reading ranges, and converting to and from different types.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::Blob;
//!
//! // Creating a new Blob from a vector of bytes
//! let vec = vec![0, 1, 2, 3, 4, 5, 6, 7];
//! let blob = Blob::from(&vec);
//! assert_eq!(blob.len(), 8);
//! assert_eq!(blob.range(2..5), &vec![2, 3, 4]);
//! assert_eq!(blob.clone().into_vec(), vec);
//!
//! // Creating a new Blob from a string
//! let text = String::from("Xylofön");
//! let blob = Blob::from(&text);
//! assert_eq!(blob.as_str(), "Xylofön");
//! ```

use super::ByteRange;
use anyhow::{Result, bail};
use std::fmt::Debug;
use std::ops::Range;
use std::path::Path;

/// A simple wrapper around [`Vec<u8>`] that provides additional methods for working with byte data.
///
/// # Examples
///
/// ```rust
/// use versatiles_core::Blob;
///
/// // Create a Blob from a string
/// let blob = Blob::from("Hello, world!");
/// assert_eq!(blob.len(), 13);
/// assert_eq!(blob.as_str(), "Hello, world!");
///
/// // Create a Blob from a byte slice
/// let bytes = &[0x41, 0x42, 0x43];
/// let blob2 = Blob::from(bytes);
/// assert_eq!(blob2.as_str(), "ABC");
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Blob(Vec<u8>);

#[allow(dead_code)]
impl Blob {
	/// Creates an empty `Blob`.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let empty_blob = Blob::new_empty();
	/// assert_eq!(empty_blob.len(), 0);
	/// assert!(empty_blob.is_empty());
	/// ```
	#[must_use]
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
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::new_sized(5);
	/// assert_eq!(blob.len(), 5);
	/// assert_eq!(blob.as_slice(), &[0, 0, 0, 0, 0]);
	/// ```
	#[must_use]
	pub fn new_sized(length: usize) -> Blob {
		Blob(vec![0u8; length])
	}

	/// Returns a byte slice from the specified `range`.
	///
	/// # Arguments
	///
	/// * `range` - The range of bytes to extract.
	///
	/// # Returns
	///
	/// A reference to the slice of bytes in the specified range.
	///
	/// # Panics
	///
	/// Panics if the specified range is out of bounds.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from(&[10, 20, 30, 40, 50]);
	/// let slice = blob.range(1..4);
	/// assert_eq!(slice, &[20, 30, 40]);
	/// ```
	#[must_use]
	pub fn range(&self, range: Range<usize>) -> &[u8] {
		&self.0[range]
	}

	/// Returns a new [`Blob`] containing the bytes in the specified [`ByteRange`].
	///
	/// # Arguments
	///
	/// * `range` - The byte range to extract, specified by offset and length.
	///
	/// # Errors
	///
	/// Returns an error if the specified range is out of bounds.
	///
	/// # Examples
	///
	/// ```rust
	/// # use versatiles_core::{Blob, ByteRange};
	/// # use anyhow::Result;
	/// #
	/// fn example() -> Result<()> {
	///     let blob = Blob::from("abcdef");
	///     let br = ByteRange { offset: 2, length: 3 };
	///     let subset = blob.read_range(&br)?;
	///     assert_eq!(subset.as_str(), "cde");
	///     Ok(())
	/// }
	/// ```
	pub fn read_range(&self, range: &ByteRange) -> Result<Blob> {
		if range.offset + range.length > self.0.len() as u64 {
			bail!("read outside range")
		}
		Ok(Blob::from(&self.0[range.as_range_usize()]))
	}

	/// Returns a reference to the underlying byte slice.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from("hello");
	/// assert_eq!(blob.as_slice(), b"hello");
	/// ```
	#[must_use]
	pub fn as_slice(&self) -> &[u8] {
		self.0.as_ref()
	}

	/// Returns a mutable reference to the underlying byte slice.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let mut blob = Blob::from("abc");
	/// let slice = blob.as_mut_slice();
	/// slice[0] = b'z';
	/// assert_eq!(blob.as_str(), "zbc");
	/// ```
	pub fn as_mut_slice(&mut self) -> &mut [u8] {
		self.0.as_mut()
	}

	/// Consumes this [`Blob`] and returns the underlying `Vec<u8>`.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from(&[1, 2, 3]);
	/// let vec = blob.into_vec();
	/// assert_eq!(vec, vec![1, 2, 3]);
	/// ```
	#[must_use]
	pub fn into_vec(self) -> Vec<u8> {
		self.0
	}

	/// Returns the underlying bytes as a string slice (`&str`), assuming they represent valid UTF-8 encoded text.
	///
	/// # Panics
	///
	/// Panics if the bytes are not valid UTF-8.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from("Xylofön");
	/// assert_eq!(blob.as_str(), "Xylofön");
	/// ```
	#[must_use]
	pub fn as_str(&self) -> &str {
		std::str::from_utf8(&self.0).expect("Blob content was not valid UTF-8")
	}

	/// Converts the [`Blob`] into a `String`, assuming it contains valid UTF-8 encoded text.
	///
	/// # Panics
	///
	/// Panics if the bytes are not valid UTF-8.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from("Hello");
	/// let s = blob.into_string();
	/// assert_eq!(s, "Hello");
	/// ```
	#[must_use]
	pub fn into_string(self) -> String {
		String::from_utf8(self.0).expect("Blob content was not valid UTF-8")
	}

	/// Returns a hexadecimal string representation of the underlying bytes, with each byte separated by a space.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from(&[0xDE, 0xAD, 0xBE, 0xEF]);
	/// assert_eq!(blob.as_hex(), "de ad be ef");
	/// ```
	#[must_use]
	pub fn as_hex(&self) -> String {
		self
			.0
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect::<Vec<_>>()
			.join(" ")
	}

	/// Returns the length of the underlying byte slice.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from("Hello");
	/// assert_eq!(blob.len(), 5);
	/// ```
	#[must_use]
	pub fn len(&self) -> u64 {
		self.0.len() as u64
	}

	/// Returns `true` if the underlying byte slice is empty, `false` otherwise.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::new_empty();
	/// assert!(blob.is_empty());
	/// ```
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	/// Saves the contents of this [`Blob`] to the given filesystem path.
	///
	/// # Arguments
	///
	/// * `path` - Destination path where the bytes will be written.
	///
	/// # Errors
	///
	/// Returns an error if the file cannot be created or written (for example due to
	/// insufficient permissions, a non‑existent parent directory, or running out of disk space).
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	/// use std::path::PathBuf;
	/// # use anyhow::Result;
	/// # fn main() -> Result<()> {
	/// let data = Blob::from(&[1u8, 2, 3, 4]);
	/// let path = PathBuf::from("tmp_blob_save.bin");
	/// data.save_to_file(&path)?;
	/// assert!(path.exists());
	/// std::fs::remove_file(&path)?;
	/// # Ok(())
	/// # }
	/// ```
	pub fn save_to_file(&self, path: &Path) -> Result<()> {
		std::fs::write(path, &self.0)?;
		Ok(())
	}

	/// Loads a [`Blob`] from the given filesystem path by reading all bytes from the file.
	///
	/// # Arguments
	///
	/// * `path` - Path to the file to read.
	///
	/// # Errors
	///
	/// Returns an error if the file cannot be opened or read (for example if the file does not
	/// exist or the process lacks permissions).
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	/// use std::path::PathBuf;
	/// # use anyhow::Result;
	/// # fn main() -> Result<()> {
	/// let original = Blob::from("Hello, files!");
	/// let path = PathBuf::from("tmp_blob_load.bin");
	/// original.save_to_file(&path)?;
	///
	/// let loaded = Blob::load_from_file(&path)?;
	/// assert_eq!(loaded.as_str(), "Hello, files!");
	///
	/// std::fs::remove_file(&path)?;
	/// # Ok(())
	/// # }
	/// ```
	pub fn load_from_file(path: &Path) -> Result<Self> {
		Ok(Blob::from(std::fs::read(path)?))
	}
}

// Conversion implementations
impl From<Vec<u8>> for Blob {
	/// Converts a `Vec<u8>` into a [`Blob`].
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from(vec![1, 2, 3]);
	/// assert_eq!(blob.len(), 3);
	/// ```
	fn from(item: Vec<u8>) -> Self {
		Blob(item)
	}
}

impl From<&Vec<u8>> for Blob {
	/// Converts a reference to a `Vec<u8>` into a [`Blob`].
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let vec = vec![4, 5, 6];
	/// let blob = Blob::from(&vec);
	/// assert_eq!(blob.len(), 3);
	/// ```
	fn from(item: &Vec<u8>) -> Self {
		Blob(item.clone())
	}
}

impl From<&[u8]> for Blob {
	/// Converts a byte slice (`&[u8]`) into a [`Blob`].
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let vec = vec![0, 1, 2];
	/// let blob = Blob::from(vec.as_slice());
	/// assert_eq!(blob.len(), 3);
	/// ```
	fn from(item: &[u8]) -> Self {
		Blob(item.to_vec())
	}
}

impl<const N: usize> From<&[u8; N]> for Blob {
	/// Converts a byte array (`&[u8; N]`) into a [`Blob`].
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from(&[0, 1, 2]);
	/// assert_eq!(blob.len(), 3);
	/// ```
	fn from(item: &[u8; N]) -> Self {
		Blob(item.to_vec())
	}
}

impl From<&str> for Blob {
	/// Converts a string slice (`&str`) into a [`Blob`].
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let blob = Blob::from("Hello, world!");
	/// assert_eq!(blob.len(), 13);
	/// ```
	fn from(item: &str) -> Self {
		Blob(item.as_bytes().to_vec())
	}
}

impl From<&String> for Blob {
	/// Converts a reference to a [`String`] into a [`Blob`].
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let s = String::from("Example");
	/// let blob = Blob::from(&s);
	/// assert_eq!(blob.as_str(), "Example");
	/// ```
	fn from(item: &String) -> Self {
		Blob(item.as_bytes().to_vec())
	}
}

impl From<String> for Blob {
	/// Converts a [`String`] into a [`Blob`].
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::Blob;
	///
	/// let s = String::from("Data");
	/// let blob = Blob::from(s);
	/// assert_eq!(blob.as_str(), "Data");
	/// ```
	fn from(item: String) -> Self {
		Blob(item.into_bytes())
	}
}

/// Implements [`Debug`] by printing the byte length and hexadecimal representation of the bytes.
impl Debug for Blob {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Blob({}): {}", self.0.len(), self.as_hex())
	}
}

/// Implements [`std::fmt::Display`] by printing the (lossy) UTF-8 interpretation of the bytes.
impl std::fmt::Display for Blob {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// Use `String::from_utf8_lossy` to avoid panicking on invalid UTF-8.
		write!(f, "{}", String::from_utf8_lossy(&self.0))
	}
}

impl Default for Blob {
	/// Returns an empty [`Blob`] by default.
	fn default() -> Self {
		Self::new_empty()
	}
}

// Allow `Blob` to be used in multithreaded scenarios
unsafe impl Send for Blob {}
unsafe impl Sync for Blob {}

#[cfg(test)]
mod tests {
	use super::*;

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
		assert_eq!(blob.range(2..5), &vec![2, 3, 4]);

		// Assert that the Blob's underlying bytes are the same as the original vector
		assert_eq!(blob.into_vec(), vec);
	}

	#[test]
	fn test_creation_and_invariants() {
		// From empty
		let empty_blob = Blob::new_empty();
		assert_eq!(empty_blob.len(), 0, "Expected empty Blob to have length 0");
		assert!(empty_blob.is_empty(), "Expected empty Blob to report is_empty() = true");

		// From a sized Blob
		let sized_blob = Blob::new_sized(5);
		assert_eq!(sized_blob.len(), 5, "Expected sized Blob to have length 5");
		assert_eq!(
			sized_blob.as_slice(),
			&[0, 0, 0, 0, 0],
			"Expected Blob to be all zeroes"
		);

		// From a vector of bytes
		let vector = vec![1, 2, 3, 4];
		let blob_from_vec = Blob::from(&vector);
		assert_eq!(
			blob_from_vec.len(),
			4,
			"Expected Blob length to match source vector length"
		);
		assert_eq!(
			blob_from_vec.as_slice(),
			vector.as_slice(),
			"Expected Blob content to match source vector"
		);

		// From a string
		let text = String::from("Hello, world!");
		let blob_from_string = Blob::from(&text);
		assert_eq!(
			blob_from_string.as_str(),
			text,
			"Expected string content to round-trip via Blob"
		);
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
		let text = String::new();

		// Assert that a Blob can be created from the empty string and correctly identified as empty
		assert!(Blob::from(&text).is_empty());
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

	#[test]
	fn test_new_empty() {
		let blob = Blob::new_empty();
		assert_eq!(blob.len(), 0);
		assert!(blob.is_empty());
	}

	#[test]
	fn test_new_sized() {
		let blob = Blob::new_sized(5);
		assert_eq!(blob.len(), 5);
		assert_eq!(blob.as_slice(), &[0, 0, 0, 0, 0]);
	}

	#[test]
	fn test_range() {
		let blob = Blob::from(&[10, 20, 30, 40, 50]);
		assert_eq!(blob.range(1..4), &[20, 30, 40]);
	}

	#[test]
	fn test_read_range() -> Result<()> {
		let blob = Blob::from("abcdef");
		let br_ok = ByteRange { offset: 2, length: 3 };
		let subset = blob.read_range(&br_ok)?;
		assert_eq!(subset.as_str(), "cde");

		let br_fail = ByteRange { offset: 4, length: 10 };
		let subset_fail = blob.read_range(&br_fail);
		assert!(
			subset_fail.is_err(),
			"Expected failure if offset+length exceeds Blob length"
		);
		Ok(())
	}

	#[test]
	fn test_read_range_error() {
		let blob = Blob::from(&[1, 2, 3]);
		let br = ByteRange { offset: 2, length: 5 };
		let result = blob.read_range(&br);
		assert!(result.is_err(), "Should fail if offset+length > blob length");
	}

	#[test]
	fn test_as_str_behavior() {
		let valid = Blob::from("Xylofön");
		assert_eq!(valid.as_str(), "Xylofön");

		let invalid_utf8 = vec![0xC3, 0x28]; // invalid sequence
		let invalid_blob = Blob::from(invalid_utf8);
		let result = std::panic::catch_unwind(|| invalid_blob.as_str());
		assert!(result.is_err());
	}

	/// Verifies that `into_string` similarly panics if the data is not valid UTF-8.
	#[test]
	fn test_into_string_behavior() {
		let valid = Blob::from("Heippa");
		let converted = valid.into_string();
		assert_eq!(
			converted, "Heippa",
			"Expected valid UTF-8 to convert into_string() successfully"
		);

		let invalid_utf8 = vec![0xFF, 0xFE, 0xFD];
		let invalid_blob = Blob::from(invalid_utf8);
		let result = std::panic::catch_unwind(|| invalid_blob.into_string());
		assert!(result.is_err(), "Expected panic for invalid UTF-8 in into_string()");
	}

	#[test]
	fn test_as_hex() {
		let blob = Blob::from(&[0xAB, 0xCD, 0xEF]);
		assert_eq!(blob.as_hex(), "ab cd ef");
	}

	#[test]
	fn test_debug_representation() {
		let blob = Blob::from("Voisilmäpulla");
		let debug_str = format!("{blob:?}");
		// Example: "Blob(14): 56 6f 69 73 69 6c 6d c3 a4 70 75 6c 6c 61"
		assert!(debug_str.starts_with("Blob(14):"));
		assert!(debug_str.contains("56 6f 69 73 69 6c 6d"));
	}

	#[test]
	fn test_display_representation() {
		let blob = Blob::from("Förstår du svenska?");
		let display_str = format!("{blob}");
		assert_eq!(display_str, "Förstår du svenska?");
	}

	#[test]
	fn test_into_vec() {
		let original = vec![1, 2, 3, 4];
		let blob = Blob::from(&original);
		let converted = blob.into_vec();
		assert_eq!(converted, original);
	}

	#[test]
	fn test_is_empty() {
		let empty_blob = Blob::new_empty();
		assert!(empty_blob.is_empty());

		let non_empty_blob = Blob::from("hello");
		assert!(!non_empty_blob.is_empty());
	}

	#[test]
	fn test_from_string() {
		let text = String::from("Hello!");
		let blob = Blob::from(text.clone());
		assert_eq!(blob.as_str(), "Hello!");

		let blob_ref = Blob::from(&text);
		assert_eq!(blob_ref.as_str(), "Hello!");
	}

	#[test]
	fn test_as_mut_slice() {
		let mut blob = Blob::from("abc");
		let slice = blob.as_mut_slice();
		slice[0] = b'z';
		assert_eq!(blob.as_str(), "zbc");
	}
}
