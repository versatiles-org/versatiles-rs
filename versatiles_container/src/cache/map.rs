//! Generic key→values cache that can live in memory or on disk.
//!
//! `CacheMap<K, V>` provides a simple append-friendly cache from a `CacheKey` to a `Vec<V>` of
//! `CacheValue`s. The concrete backend is chosen at runtime via [`CacheType`], using either
//! an in-memory map or an on-disk directory-backed store. When disk-backed, each instance gets a
//! unique subdirectory named `map_<UUID>` inside the configured cache directory.
//!
//! Typical operations are `insert`, `append`, `get_clone`, and `remove`, all of which mirror the
//! behavior of a multimap. Errors are enriched with contextual messages via `#[context(...)]`.

use crate::cache::{
	cache_in_memory::InMemoryCache,
	cache_on_disk::OnDiskCache,
	cache_type::CacheType,
	traits::{Cache, CacheKey, CacheValue},
};
use anyhow::Result;
use std::fmt::Debug;
use uuid::Uuid;
use versatiles_derive::context;

/// A runtime-selectable cache mapping keys to ordered lists of values.
pub enum CacheMap<K: CacheKey, V: CacheValue> {
	/// In-memory cache variant.
	Memory(InMemoryCache<K, V>),
	/// Disk-backed cache variant.
	Disk(OnDiskCache<K, V>),
}

impl<K: CacheKey, V: CacheValue> CacheMap<K, V> {
	/// Create a new cache using the specified cache type.
	///
	/// * `InMemory` → uses an in-process map.
	/// * `Disk(path)` → creates/uses a unique subdirectory `map_<UUID>` under `path`.
	#[must_use]
	pub fn new(cache_type: &CacheType) -> Self {
		match cache_type {
			CacheType::InMemory => Self::Memory(InMemoryCache::new()),
			CacheType::Disk(path) => {
				let random_name = format!("map_{}", Uuid::new_v4());
				Self::Disk(OnDiskCache::new(path.clone().join(random_name)))
			}
		}
	}
	/// Return `true` if a value vector is present for `key`.
	pub fn contains_key(&self, key: &K) -> bool {
		match self {
			Self::Memory(cache) => cache.contains_key(key),
			Self::Disk(cache) => cache.contains_key(key),
		}
	}

	/// Get a cloned vector of cached values for `key`.
	///
	/// Returns `Ok(None)` if the key is absent.
	#[context("Failed to get clone from cache for key: {:?}", key)]
	pub fn get_clone(&self, key: &K) -> Result<Option<Vec<V>>> {
		match self {
			Self::Memory(cache) => cache.get_clone(key),
			Self::Disk(cache) => cache.get_clone(key),
		}
	}

	/// Remove and return the cached vector for `key`, if present.
	#[context("Failed to remove from cache for key: {:?}", key)]
	pub fn remove(&mut self, key: &K) -> Result<Option<Vec<V>>> {
		match self {
			Self::Memory(cache) => cache.remove(key),
			Self::Disk(cache) => cache.remove(key),
		}
	}

	/// Insert (overwrite) the vector for `key`.
	///
	/// Replaces any previous value vector stored under the same key.
	#[context("Failed to insert into cache for key: {:?}", key)]
	pub fn insert(&mut self, key: &K, value: Vec<V>) -> Result<()> {
		match self {
			Self::Memory(cache) => cache.insert(key, value),
			Self::Disk(cache) => cache.insert(key, value),
		}
	}

	/// Append items to the existing vector for `key`, preserving order.
	///
	/// Creates a new entry if the key does not exist yet.
	#[context("Failed to append into cache for key: {:?}", key)]
	pub fn append(&mut self, key: &K, value: Vec<V>) -> Result<()> {
		match self {
			Self::Memory(cache) => cache.append(key, value),
			Self::Disk(cache) => cache.append(key, value),
		}
	}

	/// Release backend resources (e.g., flush and remove temporary files on disk).
	///
	/// Called automatically on drop; can be invoked manually to free resources sooner.
	pub fn clean_up(&mut self) {
		match self {
			Self::Memory(cache) => cache.clean_up(),
			Self::Disk(cache) => cache.clean_up(),
		}
	}
}

// Ensure resources are cleaned up when the cache goes out of scope.
impl<K: CacheKey, V: CacheValue> Drop for CacheMap<K, V> {
	fn drop(&mut self) {
		self.clean_up();
	}
}

/// Debug output indicates whether the cache is memory- or disk-backed and delegates to the backend.
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
		let mut cache = CacheMap::<String, String>::new(&cache_type);

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
