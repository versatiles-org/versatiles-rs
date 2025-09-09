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

#[cfg(test)]
mod tests {
	use super::*;

	fn roundtrip<K: CacheKey>(k: K, hash: &str) {
		assert_eq!(k.to_cache_key(), hash);
		assert_eq!(k.as_cache_key(), hash);
		assert_eq!(k.to_cache_key(), hash);
	}

	#[test]
	fn generic_works_with_string() {
		roundtrip(String::from("a=b|c=d"), "a=b|c=d");
		roundtrip(String::from("tile:z=5/x=10/y=12"), "tile:z=5/x=10/y=12");
	}

	#[test]
	fn generic_works_with_str() {
		roundtrip("a=b|c=d", "a=b|c=d");
		roundtrip("tile:z=5/x=10/y=12", "tile:z=5/x=10/y=12");
	}
}
