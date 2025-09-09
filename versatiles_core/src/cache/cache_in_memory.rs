use anyhow::Result;

use super::traits::{Cache, CacheKey, CacheValue};
use std::{collections::HashMap, marker::PhantomData};

pub struct InMemoryCache<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	data: HashMap<String, Vec<V>>,
	_marker_k: PhantomData<K>,
}

#[allow(clippy::new_without_default)]
impl<K, V> InMemoryCache<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	pub fn new() -> Self {
		Self {
			data: HashMap::new(),
			_marker_k: PhantomData,
		}
	}
}

impl<K, V> Cache<K, V> for InMemoryCache<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	fn contains_key(&self, key: &K) -> bool {
		self.data.contains_key(key.as_cache_key())
	}

	fn get_clone(&self, key: &K) -> Result<Option<Vec<V>>> {
		Ok(self.data.get(key.as_cache_key()).cloned())
	}

	fn remove(&mut self, key: &K) -> Result<Option<Vec<V>>> {
		Ok(self.data.remove(key.as_cache_key()))
	}

	fn insert(&mut self, key: &K, values: Vec<V>) -> Result<()> {
		self.data.insert(key.to_cache_key(), values);
		Ok(())
	}

	fn append(&mut self, key: &K, values: Vec<V>) -> Result<()> {
		self
			.data
			.entry(key.as_cache_key().to_string())
			.or_default()
			.extend(values);
		Ok(())
	}

	fn clean_up(&mut self) {
		self.data.clear();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn v(s: &[&str]) -> Vec<String> {
		s.iter().map(|b| b.to_string()).collect()
	}

	#[test]
	fn basic_ops_with_string_values() -> Result<()> {
		let mut cache: InMemoryCache<String, String> = InMemoryCache::new();
		let k1 = "k:1".to_string();
		let k2 = "k:2".to_string();

		// Initially empty
		assert!(!cache.contains_key(&k1));
		assert_eq!(cache.get_clone(&k1)?, None);

		// insert/get/append order
		cache.insert(&k1, v(&["a", "b"]))?;
		assert!(cache.contains_key(&k1));
		assert_eq!(cache.get_clone(&k1)?, Some(v(&["a", "b"])));
		cache.append(&k1, v(&["c"]))?;
		assert_eq!(cache.get_clone(&k1)?, Some(v(&["a", "b", "c"])));

		// second key is independent
		cache.insert(&k2, v(&["x"]))?;
		assert_eq!(cache.get_clone(&k2)?, Some(v(&["x"])));

		// remove returns previous value
		let removed = cache.remove(&k1)?;
		assert_eq!(removed, Some(v(&["a", "b", "c"])));
		assert!(!cache.contains_key(&k1));
		assert_eq!(cache.get_clone(&k1)?, None);

		// clean_up clears the entire cache in this implementation
		cache.clean_up();
		assert!(!cache.contains_key(&k2));
		assert_eq!(cache.get_clone(&k2)?, None);

		Ok(())
	}

	#[test]
	fn basic_ops_with_binary_values() -> Result<()> {
		let mut cache: InMemoryCache<String, Vec<u8>> = InMemoryCache::new();
		let k = "blob".to_string();

		assert!(!cache.contains_key(&k));
		cache.insert(&k, vec![vec![0, 1], vec![255]])?;
		assert_eq!(cache.get_clone(&k)?, Some(vec![vec![0, 1], vec![255]]));
		cache.append(&k, vec![vec![9, 9, 9]])?;
		assert_eq!(cache.get_clone(&k)?, Some(vec![vec![0, 1], vec![255], vec![9, 9, 9]]));
		let removed = cache.remove(&k)?;
		assert_eq!(removed, Some(vec![vec![0, 1], vec![255], vec![9, 9, 9]]));
		assert_eq!(cache.get_clone(&k)?, None);
		Ok(())
	}

	#[test]
	fn append_creates_entry_if_missing() -> Result<()> {
		let mut cache: InMemoryCache<String, String> = InMemoryCache::new();
		let k = "new".to_string();
		cache.append(&k, v(&["v"]))?;
		assert!(cache.contains_key(&k));
		assert_eq!(cache.get_clone(&k)?, Some(v(&["v"])));
		Ok(())
	}

	#[test]
	fn works_with_str_keys() -> Result<()> {
		let mut cache: InMemoryCache<&'static str, String> = InMemoryCache::new();
		let k: &str = "key";
		assert!(!cache.contains_key(&k));
		cache.insert(&k, v(&["a"]))?;
		assert!(cache.contains_key(&k));
		cache.append(&k, v(&["b", "c"]))?;
		assert_eq!(cache.get_clone(&k)?, Some(v(&["a", "b", "c"])));
		Ok(())
	}
}
