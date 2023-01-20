use hyper::body::Bytes;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob(Bytes);
impl Blob {
	pub fn from_bytes(bytes: Bytes) -> Blob {
		Blob(bytes)
	}
	pub fn from_vec(vec: Vec<u8>) -> Blob {
		Blob(Bytes::from(vec))
	}
	pub fn from_slice(slice: &[u8]) -> Blob {
		Blob(Bytes::copy_from_slice(slice))
	}
	pub fn empty() -> Blob {
		Blob(Bytes::from(Vec::new()))
	}
	pub fn get_range(&self, range: Range<usize>) -> Blob {
		Blob(Bytes::from(Vec::from(&self.0[range])))
	}

	pub fn as_slice(&self) -> &[u8] {
		self.0.as_ref()
	}
	pub fn as_vec(&self) -> Vec<u8> {
		self.0.to_vec()
	}

	pub fn len(&self) -> usize {
		self.0.len()
	}
}

#[cfg(test)]
mod tests {
	use super::Blob;

	#[test]
	fn basic_tests() {
		let vec = vec![0, 1, 2, 3, 4, 5, 6, 7];
		let blob = Blob::from_vec(vec.clone());
		assert_eq!(blob.as_vec(), vec);
		assert_eq!(Blob::from_slice(blob.as_slice()), blob);
		assert_eq!(blob.len(), 8);
		assert_eq!(blob.get_range(2..5).as_vec(), vec![2, 3, 4]);
	}
}
