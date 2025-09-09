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
