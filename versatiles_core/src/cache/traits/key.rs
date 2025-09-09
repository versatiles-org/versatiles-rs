pub trait CacheKey {
	fn as_cache_key(&self) -> &str;
	fn to_cache_key(&self) -> String;
}

impl CacheKey for String {
	fn as_cache_key(&self) -> &str {
		self
	}
	fn to_cache_key(&self) -> String {
		self.clone()
	}
}

impl CacheKey for &str {
	fn as_cache_key(&self) -> &str {
		self
	}
	fn to_cache_key(&self) -> String {
		self.to_string()
	}
}
