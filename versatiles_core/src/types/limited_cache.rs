//! This module provides a generic limited cache that stores key-value pairs up to a specified byte size limit.
//!
//! The `LimitedCache` manages entries in a manner resembling an LRU cache, ensuring it does not exceed
//! a predefined number of elements (derived from the byte size limit). Once the limit is reached,
//! least-recently accessed items are removed automatically.

use anyhow::Result;
use lru::LruCache;
use std::{fmt::Debug, hash::Hash, mem::size_of, num::NonZeroUsize, ops::Div};
use versatiles_derive::context;

/// A generic cache that stores key-value pairs up to a specified total size limit (in bytes).
///
/// The cache uses a least-recently-used (LRU) strategy when it needs to remove items.
/// When the cache is at capacity, the least recently accessed item is automatically evicted.
///
/// # Type Parameters
/// - `K`: The type of the keys stored in the cache. Must implement `Eq + Hash + Clone`.
/// - `V`: The type of the values stored in the cache. Must implement `Clone`.
///
/// # Examples
///
/// ```rust
/// use versatiles_core::LimitedCache;
///
/// // Create a cache with a maximum byte size of 1,000,000.
/// // The actual number of elements that can be stored depends on
/// // the size of (K, V).
/// let mut cache = LimitedCache::<i32, u64>::with_maximum_size(1_000_000);
///
/// // Insert some data
/// cache.add(1, 42);
///
/// // Retrieve data
/// assert_eq!(cache.get(&1), Some(42));
/// ```
pub struct LimitedCache<K, V> {
	/// Internal LRU cache storing key-value pairs.
	cache: LruCache<K, V>,
}

impl<K, V> LimitedCache<K, V>
where
	K: Clone + Debug + Eq + Hash + PartialEq,
	V: Clone,
{
	/// Creates a new `LimitedCache` with a specified maximum **byte** size.
	///
	/// Internally, it computes how many `(K, V)` pairs can fit into that byte size,
	/// based on `size_of::<K>() + size_of::<V>()`.
	///
	/// # Arguments
	/// * `maximum_size` - The total byte size the cache is allowed to occupy.
	///
	/// # Panics
	///
	/// Panics if `maximum_size` is too small to store even a single `(K, V)` pair.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::LimitedCache;
	///
	/// let cache: LimitedCache<u64, i32> = LimitedCache::with_maximum_size(1024);
	/// ```
	#[must_use]
	pub fn with_maximum_size(maximum_size: usize) -> Self {
		// Compute how many (K, V) pairs can fit into `maximum_size`.
		let per_element_size = size_of::<K>() + size_of::<V>();
		let max_length = maximum_size.div(per_element_size);
		assert!(
			max_length > 0,
			"size ({maximum_size} bytes) is too small to store a single element of size {per_element_size} bytes"
		);

		Self {
			cache: LruCache::new(NonZeroUsize::new(max_length).unwrap()),
		}
	}

	/// Retrieves a cloned value from the cache by its key, updating the last access time.
	///
	/// If the key exists:
	/// - The method marks this key as most recently used.
	/// - Returns a copy of the stored value.
	///
	/// If the key does not exist, returns `None`.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::LimitedCache;
	///
	/// let mut cache = LimitedCache::with_maximum_size(1_000);
	/// cache.add("foo", 42);
	/// assert_eq!(cache.get(&"foo"), Some(42));
	/// assert_eq!(cache.get(&"bar"), None);
	/// ```
	pub fn get(&mut self, key: &K) -> Option<V> {
		self.cache.get(key).cloned()
	}

	/// Gets the value corresponding to `key` if it exists; otherwise calls the given callback
	/// to produce the value, stores it in the cache, and returns it.
	///
	/// # Errors
	///
	/// If the callback returns an error (`anyhow::Error`), this method propagates that error.
	///
	/// # Examples
	///
	/// ```rust
	/// # use versatiles_core::LimitedCache;
	/// # use anyhow::{anyhow, Result};
	/// fn expensive_operation() -> Result<u64> {
	///     Ok(999)
	/// }
	///
	/// fn example() -> Result<()> {
	///     let mut cache = LimitedCache::with_maximum_size(1_000);
	///     let val = cache.get_or_set(&"key", || expensive_operation())?;
	///     assert_eq!(val, 999);
	///
	///     // A second call should retrieve from cache
	///     let val_again = cache.get(&"key");
	///     assert_eq!(val_again, Some(999));
	///     Ok(())
	/// }
	/// ```
	#[context("Could not get or set cache value for key '{:?}'", key)]
	pub fn get_or_set<F>(&mut self, key: &K, callback: F) -> Result<V>
	where
		F: FnOnce() -> Result<V>,
	{
		if let Some(cached) = self.get(key) {
			return Ok(cached);
		}

		// Cache miss
		let value = callback()?;
		let cloned_value = value.clone();
		self.add(key.clone(), value);
		Ok(cloned_value)
	}

	/// Adds a new `key -> value` pair to the cache, returning the inserted value.
	///
	/// - If adding triggers the capacity limit, the least recently used item is automatically evicted.
	/// - The newly added item becomes the most recently used item.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::LimitedCache;
	///
	/// let mut cache = LimitedCache::with_maximum_size(1_000);
	/// let inserted = cache.add("foo", 123);
	/// assert_eq!(inserted, 123);
	/// ```
	pub fn add(&mut self, key: K, value: V) -> V {
		let cloned_value = value.clone();
		self.cache.put(key, value);
		cloned_value
	}

	/// Returns the current number of entries in the cache.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::LimitedCache;
	///
	/// let mut cache = LimitedCache::with_maximum_size(1_000);
	/// assert_eq!(cache.len(), 0);
	/// cache.add("foo", 42);
	/// assert_eq!(cache.len(), 1);
	/// ```
	pub fn len(&self) -> usize {
		self.cache.len()
	}

	/// Returns true if the cache contains no entries.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::LimitedCache;
	///
	/// let mut cache = LimitedCache::with_maximum_size(1_000);
	/// assert!(cache.is_empty());
	/// cache.add("foo", 42);
	/// assert!(!cache.is_empty());
	/// ```
	pub fn is_empty(&self) -> bool {
		self.cache.is_empty()
	}

	/// Returns the maximum capacity of the cache.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::LimitedCache;
	///
	/// let cache: LimitedCache<u64, u64> = LimitedCache::with_maximum_size(1_000);
	/// // Capacity depends on element size
	/// assert!(cache.capacity() > 0);
	/// ```
	pub fn capacity(&self) -> usize {
		self.cache.cap().get()
	}
}

impl<K, V> Debug for LimitedCache<K, V>
where
	K: Clone + Debug + Eq + Hash + PartialEq,
	V: Clone,
{
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("LimitedCache")
			.field("length", &self.len())
			.field("max_length", &self.capacity())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::LimitedCache;
	use anyhow::{Result, anyhow};
	use std::mem::size_of;

	/// Ensures that creation with a given `maximum_size` sets the derived capacity appropriately.
	#[test]
	fn test_cache_initialization() {
		// Each (u64, i32) pair consumes size_of::<u64>() + size_of::<i32>() bytes.
		let element_size = size_of::<u64>() + size_of::<i32>();
		// Suppose we allow 100 bytes.
		let maximum_size = 100;
		let cache: LimitedCache<u64, i32> = LimitedCache::with_maximum_size(maximum_size);
		let expected_max_len = maximum_size / element_size;
		assert_eq!(cache.capacity(), expected_max_len);
	}

	/// Ensures that we can store and retrieve values, and `None` is returned for absent keys.
	#[test]
	fn test_add_and_get_items() {
		let mut cache = LimitedCache::with_maximum_size(10 * (size_of::<i32>() + size_of::<i32>()));
		cache.add(1, 100);
		cache.add(2, 200);

		assert_eq!(cache.get(&1), Some(100));
		assert_eq!(cache.get(&2), Some(200));
		assert_eq!(cache.get(&3), None);
	}

	/// Checks that `get_or_set` retrieves existing data or inserts a new value if missing.
	#[test]
	fn test_get_or_set() -> Result<()> {
		let mut cache = LimitedCache::with_maximum_size(10 * (size_of::<i32>() + size_of::<i32>()));

		// Key does not exist, so callback is called
		let val = cache.get_or_set(&1, || Ok(999))?;
		assert_eq!(val, 999);
		// Now it's cached
		assert_eq!(cache.get(&1), Some(999));

		// Next call shouldn't invoke the callback
		let val2 = cache.get_or_set(&1, || Err(anyhow!("Should not be called")))?;
		assert_eq!(val2, 999);

		Ok(())
	}

	/// Verifies that the LRU eviction works correctly when the cache hits capacity.
	#[test]
	fn test_capacity_and_lru_eviction() {
		// Create a cache that can hold exactly 5 u64 pairs
		let mut cache: LimitedCache<u64, u64> = LimitedCache::with_maximum_size(5 * 2 * std::mem::size_of::<u64>());

		// Add 5 items (0..4)
		for i in 0..5 {
			cache.add(i, i * 100);
		}

		// All 5 should be present
		assert_eq!(cache.len(), 5);
		for i in 0..5 {
			assert_eq!(cache.get(&i), Some(i * 100));
		}

		// Access item 0 to make it most recently used
		let _ = cache.get(&0);

		// Add a new item (5), which should evict the LRU item (1, since 0 was just accessed)
		cache.add(5, 500);

		// Now we should have: 0, 2, 3, 4, 5 (1 was evicted)
		assert_eq!(cache.len(), 5);
		assert_eq!(cache.get(&0), Some(0)); // Still present (was accessed)
		assert_eq!(cache.get(&1), None); // Evicted (was LRU)
		assert_eq!(cache.get(&2), Some(200));
		assert_eq!(cache.get(&3), Some(300));
		assert_eq!(cache.get(&4), Some(400));
		assert_eq!(cache.get(&5), Some(500));
	}

	/// Tests that accessing items updates their LRU position
	#[test]
	fn test_lru_updates_on_access() {
		let mut cache: LimitedCache<u64, u64> = LimitedCache::with_maximum_size(3 * 2 * std::mem::size_of::<u64>());

		// Add 3 items
		cache.add(1, 100);
		cache.add(2, 200);
		cache.add(3, 300);

		// Access item 1 to make it most recently used
		let _ = cache.get(&1);

		// Add a new item, should evict 2 (the LRU)
		cache.add(4, 400);

		assert_eq!(cache.get(&1), Some(100)); // Still present
		assert_eq!(cache.get(&2), None); // Evicted
		assert_eq!(cache.get(&3), Some(300)); // Still present
		assert_eq!(cache.get(&4), Some(400)); // Newly added
	}

	/// Ensures that `with_maximum_size` panics if the size is too small to store even a single `(K, V)`.
	#[test]
	#[should_panic(expected = "size")]
	fn test_creation_too_small() {
		// For (u8, u8), each pair is 2 bytes in memory.
		// Let's specify a limit < 2
		let _cache: LimitedCache<u8, u8> = LimitedCache::with_maximum_size(1);
	}

	/// Simple test of the Debug trait output.
	#[test]
	fn test_debug_format() {
		let cache: LimitedCache<u8, u8> = LimitedCache::with_maximum_size(10);
		let debug_str = format!("{cache:?}");
		// Example: "LimitedCache { length: 0, max_length: 5 }"
		assert!(debug_str.contains("LimitedCache"));
		assert!(debug_str.contains("length"));
		assert!(debug_str.contains("max_length"));
	}

	/// Test that the cache properly handles capacity
	#[test]
	fn test_capacity_methods() {
		let mut cache: LimitedCache<u64, u64> = LimitedCache::with_maximum_size(3 * 2 * std::mem::size_of::<u64>());

		assert_eq!(cache.capacity(), 3);
		assert_eq!(cache.len(), 0);
		assert!(cache.is_empty());

		cache.add(1, 100);
		assert_eq!(cache.len(), 1);
		assert!(!cache.is_empty());

		cache.add(2, 200);
		cache.add(3, 300);
		assert_eq!(cache.len(), 3);

		// Adding one more should evict the LRU
		cache.add(4, 400);
		assert_eq!(cache.len(), 3); // Still at capacity
	}
}
