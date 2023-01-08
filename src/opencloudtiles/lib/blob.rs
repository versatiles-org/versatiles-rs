use hyper::body::Bytes;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct Blob(Bytes);
impl Blob {
	pub fn from_vec(vec: Vec<u8>) -> Blob {
		return Blob(Bytes::from(vec));
	}
	pub fn from_slice(slice: &[u8]) -> Blob {
		return Blob(Bytes::copy_from_slice(slice));
	}
	pub fn empty() -> Blob {
		return Blob(Bytes::from(Vec::new()));
	}
	pub fn get_range(&self, range: Range<usize>) -> Blob {
		return Blob(Bytes::from(Vec::from(&self.0[range])));
	}

	pub fn as_slice(&self) -> &[u8] {
		return self.0.as_ref();
	}
	pub fn to_vec(&self) -> Vec<u8> {
		return self.0.to_vec();
	}

	pub fn len(&self) -> usize {
		return self.0.len();
	}
}
