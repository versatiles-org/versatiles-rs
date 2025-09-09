use anyhow::Result;

use crate::{
	cache::{
		cache_in_memory::InMemoryCache,
		cache_on_disk::OnDiskCache,
		traits::{Cache, CacheKey, CacheValue},
	},
	config::CacheKind,
};

pub enum CacheMap<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	Memory(InMemoryCache<K, V>),
	Disk(OnDiskCache<K, V>),
}

impl<K, V> CacheMap<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	pub fn new(kind: &CacheKind) -> Self {
		match kind {
			CacheKind::InMemory => Self::Memory(InMemoryCache::new()),
			CacheKind::Disk(path) => Self::Disk(OnDiskCache::new(path.clone())),
		}
	}
}

impl<K, V> Cache<K, V> for CacheMap<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	fn contains_key(&self, key: &K) -> bool {
		match self {
			Self::Memory(cache) => cache.contains_key(key),
			Self::Disk(cache) => cache.contains_key(key),
		}
	}

	fn get_clone(&self, key: &K) -> Result<Option<Vec<V>>> {
		match self {
			Self::Memory(cache) => cache.get_clone(key),
			Self::Disk(cache) => cache.get_clone(key),
		}
	}

	fn remove(&mut self, key: &K) -> Result<Option<Vec<V>>> {
		match self {
			Self::Memory(cache) => cache.remove(key),
			Self::Disk(cache) => cache.remove(key),
		}
	}

	fn insert(&mut self, key: &K, value: Vec<V>) -> Result<()> {
		match self {
			Self::Memory(cache) => cache.insert(key, value),
			Self::Disk(cache) => cache.insert(key, value),
		}
	}

	fn append(&mut self, key: &K, value: Vec<V>) -> Result<()> {
		match self {
			Self::Memory(cache) => cache.append(key, value),
			Self::Disk(cache) => cache.append(key, value),
		}
	}

	fn clean_up(&mut self) {
		match self {
			Self::Memory(cache) => cache.clean_up(),
			Self::Disk(cache) => cache.clean_up(),
		}
	}
}

impl<K, V> Drop for CacheMap<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	fn drop(&mut self) {
		self.clean_up();
	}
}
