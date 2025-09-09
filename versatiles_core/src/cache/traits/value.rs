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

#[cfg(test)]
mod tests {
	use super::*;

	fn roundtrip<T>(value: T)
	where
		T: CacheValue + PartialEq + core::fmt::Debug,
	{
		let buf = value.to_cache_buffer();
		let decoded = T::from_cache_buffer(&buf);
		assert_eq!(decoded, value);
	}

	#[test]
	fn vec_u8_roundtrips_various_payloads() {
		roundtrip::<Vec<u8>>(vec![]);
		roundtrip::<Vec<u8>>(vec![0]);
		roundtrip::<Vec<u8>>(vec![0, 1, 2, 3, 4, 5]);
		// include non-UTF8 bytes to ensure raw bytes are preserved
		roundtrip::<Vec<u8>>(vec![0, 255, 128, 10, 200, 0]);
	}

	#[test]
	fn string_roundtrips_ascii_and_unicode() {
		// ASCII
		roundtrip::<String>("hello world".to_string());
		// Unicode with multi-byte code points
		roundtrip::<String>("Grüße 🌍 — こんにちは".to_string());

		// Buffer content matches underlying UTF-8 bytes
		let s = "naïve café".to_string();
		let buf = s.to_cache_buffer();
		assert_eq!(buf, s.as_bytes());
	}

	#[test]
	#[should_panic]
	fn string_from_cache_buffer_panics_on_invalid_utf8() {
		// Construct a buffer that is not valid UTF-8 (single 0xFF byte)
		let invalid = [0xFFu8, 0xFEu8, 0x00u8];
		// This should panic because the String impl unwraps the UTF-8 conversion
		let _ = String::from_cache_buffer(&invalid);
	}
}
