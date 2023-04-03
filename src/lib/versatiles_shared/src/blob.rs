use bytes::Bytes;
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
	pub fn from_str_ref(text: &str) -> Blob {
		Blob(Bytes::from(text.to_owned()))
	}
	pub fn from_string(text: String) -> Blob {
		Blob(Bytes::from(text))
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
	pub fn as_str(&self) -> &str {
		std::str::from_utf8(&self.0).unwrap()
	}
	pub fn len(&self) -> usize {
		self.0.len()
	}
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

unsafe impl Send for Blob {}
unsafe impl Sync for Blob {}

#[cfg(test)]
mod tests {
	use bytes::Bytes;

	use super::Blob;

	#[test]
	fn basic_tests() {
		let vec = vec![0, 1, 2, 3, 4, 5, 6, 7];
		let blob = Blob::from_vec(vec.clone());
		assert_eq!(blob.as_vec(), vec);
		assert_eq!(blob.len(), 8);
		assert_eq!(blob.get_range(2..5).as_vec(), vec![2, 3, 4]);
	}

	#[test]
	fn string() {
		let text = String::from("Xylofön");
		assert_eq!(Blob::from_str_ref(&text).as_str(), text);
		assert_eq!(Blob::from_string(text.clone()).as_str(), text);
	}

	#[test]
	fn empty() {
		let text = String::from("");
		assert_eq!(Blob::from_str_ref(&text).is_empty(), true);
	}

	#[test]
	fn bytes() {
		let text = String::from("Smørrebrød");
		let bytes = Bytes::from(text.clone());
		assert_eq!(Blob::from_bytes(bytes).as_str(), text);
	}

	#[test]
	fn debug() {
		let text = String::from("Voisilmäpulla");
		let blob = Blob::from_str_ref(&text);
		let debug = format!("{:?}", blob);
		println!("{}", debug);
		//assert_eq!(format!("{:}blob Blob::from_bytes(bytes).as_str(), text);
	}
}
