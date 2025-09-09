pub trait CacheValue: Clone {
	fn to_cache_buffer(&self) -> Vec<u8>;
	fn from_cache_buffer(buf: &[u8]) -> Self;
}

impl CacheValue for Vec<u8> {
	fn to_cache_buffer(&self) -> Vec<u8> {
		self.clone()
	}
	fn from_cache_buffer(buf: &[u8]) -> Self {
		buf.to_vec()
	}
}

impl CacheValue for String {
	fn to_cache_buffer(&self) -> Vec<u8> {
		self.as_bytes().to_vec()
	}
	fn from_cache_buffer(buf: &[u8]) -> Self {
		String::from_utf8(buf.to_vec()).unwrap()
	}
}
