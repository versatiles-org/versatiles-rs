use crate::{
	cache::{
		cache_in_memory::InMemoryCache,
		cache_on_disk::OnDiskCache,
		traits::{Cache, CacheKey, CacheValue},
	},
	config::{CacheType, Config},
};
use anyhow::Result;
use std::{fmt::Debug, sync::Arc};
use uuid::Uuid;
use versatiles_derive::context;

pub enum CacheMap<K: CacheKey, V: CacheValue> {
	Memory(InMemoryCache<K, V>),
	Disk(OnDiskCache<K, V>),
}

impl<K: CacheKey, V: CacheValue> CacheMap<K, V> {
	#[must_use]
	pub fn new(config: Arc<Config>) -> Self {
		match &config.cache_type {
			CacheType::InMemory => Self::Memory(InMemoryCache::new()),
			CacheType::Disk(path) => {
				let random_name = format!("map_{}", Uuid::new_v4());
				Self::Disk(OnDiskCache::new(path.clone().join(random_name)))
			}
		}
	}
	pub fn contains_key(&self, key: &K) -> bool {
		match self {
			Self::Memory(cache) => cache.contains_key(key),
			Self::Disk(cache) => cache.contains_key(key),
		}
	}

	#[context("Failed to get clone from cache for key: {:?}", key)]
	pub fn get_clone(&self, key: &K) -> Result<Option<Vec<V>>> {
		match self {
			Self::Memory(cache) => cache.get_clone(key),
			Self::Disk(cache) => cache.get_clone(key),
		}
	}

	#[context("Failed to remove from cache for key: {:?}", key)]
	pub fn remove(&mut self, key: &K) -> Result<Option<Vec<V>>> {
		match self {
			Self::Memory(cache) => cache.remove(key),
			Self::Disk(cache) => cache.remove(key),
		}
	}

	#[context("Failed to insert into cache for key: {:?}", key)]
	pub fn insert(&mut self, key: &K, value: Vec<V>) -> Result<()> {
		match self {
			Self::Memory(cache) => cache.insert(key, value),
			Self::Disk(cache) => cache.insert(key, value),
		}
	}

	#[context("Failed to append into cache for key: {:?}", key)]
	pub fn append(&mut self, key: &K, value: Vec<V>) -> Result<()> {
		match self {
			Self::Memory(cache) => cache.append(key, value),
			Self::Disk(cache) => cache.append(key, value),
		}
	}

	pub fn clean_up(&mut self) {
		match self {
			Self::Memory(cache) => cache.clean_up(),
			Self::Disk(cache) => cache.clean_up(),
		}
	}
}

impl<K: CacheKey, V: CacheValue> Drop for CacheMap<K, V> {
	fn drop(&mut self) {
		self.clean_up();
	}
}

impl<K: CacheKey, V: CacheValue> Debug for CacheMap<K, V> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Memory(cache) => write!(f, "CacheMap::Memory({cache:?})"),
			Self::Disk(cache) => write!(f, "CacheMap::Disk({cache:?})"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;
	use tempfile::TempDir;

	fn string_vec(s: &str) -> Vec<String> {
		s.split(',').map(|b| b.trim().to_string()).collect()
	}

	#[rstest]
	#[case::mem("mem")]
	#[case::disk("disk")]
	fn test_cache_type(#[case] case: &str) -> Result<()> {
		let cache_type = match case {
			"mem" => CacheType::InMemory,
			"disk" => CacheType::Disk(TempDir::new().unwrap().path().to_path_buf()),
			_ => panic!("unknown cache kind"),
		};
		let config = Arc::new(Config { cache_type });
		let mut cache = CacheMap::<String, String>::new(config);

		let k1 = "k:1".to_string();
		let k2 = "k:2".to_string();

		// Initially empty
		assert!(!cache.contains_key(&k1));
		assert_eq!(cache.get_clone(&k1)?, None);
		assert_eq!(cache.remove(&k1)?, None);

		// Insert a vector of values
		cache.insert(&k1, string_vec("a,b"))?;
		assert!(cache.contains_key(&k1));
		assert_eq!(cache.get_clone(&k1)?, Some(string_vec("a,b")));

		// Append preserves order
		cache.append(&k1, string_vec("c"))?;
		assert!(cache.contains_key(&k1));
		assert_eq!(cache.get_clone(&k1)?, Some(string_vec("a,b,c")));

		// Second key remains independent
		assert!(!cache.contains_key(&k2));
		cache.insert(&k2, string_vec("x"))?;
		assert_eq!(cache.get_clone(&k2)?, Some(string_vec("x")));

		// Append preserves order
		cache.append(&k2, string_vec("y,z"))?;
		assert!(cache.contains_key(&k2));
		assert_eq!(cache.get_clone(&k2)?, Some(string_vec("x,y,z")));

		// Remove returns previous value and clears the key
		let removed = cache.remove(&k1)?;
		assert_eq!(removed, Some(string_vec("a,b,c")));
		assert!(!cache.contains_key(&k1));
		assert_eq!(cache.get_clone(&k1)?, None);

		// Clean up should remove all
		cache.clean_up();
		assert!(!cache.contains_key(&k1));
		assert!(!cache.contains_key(&k2));

		Ok(())
	}
}
