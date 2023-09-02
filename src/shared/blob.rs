#![allow(dead_code)]

use bytes::Bytes;
use std::fmt::Debug;
use std::ops::Range;

/// A simple wrapper around `bytesMut::Bytes` that provides additional methods for working with byte data.
#[derive(Clone, PartialEq, Eq)]
pub struct Blob(Vec<u8>);

impl Blob {
	/// Creates an empty `Blob`.
	pub fn empty() -> Blob {
		Blob(Vec::new())
	}

	/// Returns a new `Blob` containing the bytes in the specified range.
	pub fn get_range(&self, range: Range<usize>) -> &[u8] {
		&self.0[range]
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
	pub fn as_vec(&self) -> Vec<u8> {
		self.0.to_vec()
	}

	/// Returns the underlying bytes as a string, assuming they represent valid UTF-8 encoded text.
	pub fn as_str(&self) -> &str {
		std::str::from_utf8(&self.0).unwrap()
	}

	/// Returns the length of the underlying byte slice.
	pub fn len(&self) -> usize {
		self.0.len()
	}

	/// Returns `true` if the underlying byte slice is empty, `false` otherwise.
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

/*
impl From<BytesMut> for Blob {
	/// Converts a `bytes::BytesMut` instance into a `Blob`.
	fn from(item: BytesMut) -> Self {
		Blob(item)
	}
}
*/

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
	/// Converts a `Vec<u8>` instance into a `Blob`.
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
		Blob(item.as_bytes().to_vec())
	}
}

impl Debug for Blob {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let chunk: &[u8];
		let mut suffix: &str = "";
		if self.0.len() > 32 {
			suffix = "...";
			chunk = self.0.get(0..32).unwrap();
		} else {
			chunk = &self.0;
		}
		write!(
			f,
			"Blob({}, {}{})",
			self.0.len(),
			chunk
				.iter()
				.map(|v| format!("{v:X}"))
				.collect::<Vec<String>>()
				.join("_"),
			suffix
		)
	}
}

impl ToString for Blob {
	fn to_string(&self) -> String {
		String::from_utf8_lossy(&self.0).to_string()
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

		// Assert that the Blob's underlying bytes are the same as the original vector
		assert_eq!(blob.as_vec(), vec);

		// Assert that the Blob's length is correct
		assert_eq!(blob.len(), 8);

		// Assert that a range of bytes can be extracted from the Blob correctly
		assert_eq!(blob.get_range(2..5), &vec![2, 3, 4]);
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
			"Blob(14, 56_6F_69_73_69_6C_6D_C3_A4_70_75_6C_6C_61)"
		);

		assert_eq!(
			format!("{:?}", Blob::from("01234567890123456789012345678901")),
			"Blob(32, 30_31_32_33_34_35_36_37_38_39_30_31_32_33_34_35_36_37_38_39_30_31_32_33_34_35_36_37_38_39_30_31)"
		);
		assert_eq!(
			format!("{:?}", Blob::from("012345678901234567890123456789012")),
			"Blob(33, 30_31_32_33_34_35_36_37_38_39_30_31_32_33_34_35_36_37_38_39_30_31_32_33_34_35_36_37_38_39_30_31...)"
		);
	}
}
