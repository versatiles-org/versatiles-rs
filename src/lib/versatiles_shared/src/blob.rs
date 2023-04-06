use bytes::Bytes;
use std::ops::Range;

/// A simple wrapper around `bytes::Bytes` that provides additional methods for working with byte data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob(Bytes);

impl Blob {
	/// Creates an empty `Blob`.
	pub fn empty() -> Blob {
		Blob(Bytes::from(Vec::new()))
	}

	/// Returns a new `Blob` containing the bytes in the specified range.
	pub fn get_range(&self, range: Range<usize>) -> Blob {
		Blob(Bytes::from(Vec::from(&self.0[range])))
	}

	/// Returns a reference to the underlying byte slice.
	pub fn as_slice(&self) -> &[u8] {
		self.0.as_ref()
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

impl From<Bytes> for Blob {
	/// Converts a `bytes::Bytes` instance into a `Blob`.
	fn from(item: Bytes) -> Self {
		Blob(item)
	}
}

impl From<Vec<u8>> for Blob {
	/// Converts a `Vec<u8>` instance into a `Blob`.
	fn from(item: Vec<u8>) -> Self {
		Blob(Bytes::from(item))
	}
}

impl From<&str> for Blob {
	/// Converts a `&str` instance into a `Blob`.
	fn from(item: &str) -> Self {
		Blob(Bytes::from(item.to_owned()))
	}
}

impl From<&String> for Blob {
	/// Converts a `&String` instance into a `Blob`.
	fn from(item: &String) -> Self {
		Blob(Bytes::from(item.to_owned()))
	}
}

impl From<String> for Blob {
	/// Converts a `String` instance into a `Blob`.
	fn from(item: String) -> Self {
		Blob(Bytes::from(item))
	}
}

impl From<&[u8]> for Blob {
	/// Converts a `&[u8]` instance into a `Blob`.
	fn from(item: &[u8]) -> Self {
		Blob(Bytes::from(item.to_vec()))
	}
}

unsafe impl Send for Blob {}
unsafe impl Sync for Blob {}

// module containing unit tests for Blob struct
#[cfg(test)]
mod tests {
	// Import the Blob struct from the parent module
	use super::Blob;
	use bytes::Bytes;

	// Test basic functionality of the Blob struct
	#[test]
	fn basic_tests() {
		// Create a vector of bytes
		let vec = vec![0, 1, 2, 3, 4, 5, 6, 7];

		// Create a Blob instance from the vector
		let blob = Blob::from(vec.clone());

		// Assert that the Blob's underlying bytes are the same as the original vector
		assert_eq!(blob.as_vec(), vec);

		// Assert that the Blob's length is correct
		assert_eq!(blob.len(), 8);

		// Assert that a range of bytes can be extracted from the Blob correctly
		assert_eq!(blob.get_range(2..5).as_vec(), vec![2, 3, 4]);
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

		// Assert that a Blob can be created from a reference to the string's underlying buffer and converted back to a string correctly
		assert_eq!(Blob::from(&*text).as_str(), text);
	}

	// Test creating an empty Blob
	#[test]
	fn empty() {
		// Create an empty string
		let text = String::from("");

		// Assert that a Blob can be created from the empty string and correctly identified as empty
		assert_eq!(Blob::from(&text).is_empty(), true);
	}

	// Test creating a Blob from bytes
	#[test]
	fn bytes() {
		// Create a string with non-ASCII characters
		let text = String::from("Smørrebrød");

		// Create a Bytes instance from the string
		let bytes = Bytes::from(text.clone());

		// Assert that a Blob can be created from the Bytes instance and converted back to the string correctly
		assert_eq!(Blob::from(bytes).as_str(), text);
	}

	// Test the debug format of the Blob struct
	#[test]
	fn debug() {
		// Create a string with non-ASCII characters
		let text = String::from("Voisilmäpulla");

		// Create a Blob instance from the string
		let blob = Blob::from(&text);

		// Format the Blob instance using the debug formatter and print it
		let debug = format!("{:?}", blob);
		println!("{}", debug);
	}
}
