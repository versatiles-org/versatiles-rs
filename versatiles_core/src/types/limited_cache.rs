//! This module provides a generic limited cache that stores key-value pairs up to a specified byte size limit.
//!
//! The `LimitedCache` manages entries in a manner resembling an LRU cache, ensuring it does not exceed
//! a predefined number of elements (derived from the byte size limit). Once the limit is reached,
//! least-recently accessed items are removed using a custom cleanup method.

use anyhow::Result;
use std::{collections::HashMap, fmt::Debug, hash::Hash, mem::size_of, ops::Div};

/// A generic cache that stores key-value pairs up to a specified total size limit (in bytes).
///
/// The cache uses a least-recently-used (LRU) strategy when it needs to remove items.
/// Specifically, when the cache is at capacity, it calls [`cleanup`](Self::cleanup) to evict
/// entries whose access index is at or below the computed median.
///
/// # Type Parameters
/// - `K`: The type of the keys stored in the cache. Must implement `Eq + Hash + Clone`.
/// - `V`: The type of the values stored in the cache. Must implement `Clone`.
///
/// # Examples
///
/// ```rust
/// use versatiles_core::types::LimitedCache;
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
	/// Internal map storing (value, "last access index") pairs.
	cache: HashMap<K, (V, u64)>,
	/// Derived maximum number of elements the cache can hold.
	max_length: usize,
	/// A monotonically increasing index to track access recency.
	last_index: u64,
}

impl<K, V> LimitedCache<K, V>
where
	K: Clone + Eq + Hash + PartialEq,
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
	/// use versatiles_core::types::LimitedCache;
	///
	/// let cache: LimitedCache<u64, i32> = LimitedCache::with_maximum_size(1024);
	/// ```
	pub fn with_maximum_size(maximum_size: usize) -> Self {
		// Compute how many (K, V) pairs can fit into `maximum_size`.
		let per_element_size = size_of::<K>() + size_of::<V>();
		let max_length = maximum_size.div(per_element_size);
		if max_length < 1 {
			panic!("size ({maximum_size} bytes) is too small to store a single element of size {per_element_size} bytes");
		}

		Self {
			cache: HashMap::new(),
			max_length,
			last_index: 0,
		}
	}

	/// Retrieves a cloned value from the cache by its key, updating the last access time.
	///
	/// If the key exists:
	/// - The method increments the internal `last_index`.
	/// - Updates the stored access index to reflect this more recent use.
	/// - Returns a copy of the stored value.
	///
	/// If the key does not exist, returns `None`.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::types::LimitedCache;
	///
	/// let mut cache = LimitedCache::with_maximum_size(1_000);
	/// cache.add("foo", 42);
	/// assert_eq!(cache.get(&"foo"), Some(42));
	/// assert_eq!(cache.get(&"bar"), None);
	/// ```
	pub fn get(&mut self, key: &K) -> Option<V> {
		if let Some((value, old_index)) = self.cache.get_mut(key) {
			self.last_index += 1;
			*old_index = self.last_index;
			Some(value.clone())
		} else {
			None
		}
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
	/// # use versatiles_core::types::LimitedCache;
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
	/// - Increments `last_index`.
	/// - Stores `(value, last_index)` in the internal map.  
	/// - If adding triggers the capacity limit, it runs `cleanup()` to evict items.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::types::LimitedCache;
	///
	/// let mut cache = LimitedCache::with_maximum_size(1_000);
	/// let inserted = cache.add("foo", 123);
	/// assert_eq!(inserted, 123);
	/// ```
	pub fn add(&mut self, key: K, value: V) -> V {
		if self.cache.len() >= self.max_length {
			self.cleanup();
		}

		self.last_index += 1;
		// Insert or replace. The 0.0 clone is just to ensure a consistent return type
		self.cache.entry(key).or_insert((value, self.last_index)).0.clone()
	}

	/// Removes the least recently accessed items if the cache has reached capacity.
	///
	/// The current implementation:
	/// 1. Collects all access indices into a `Vec`.
	/// 2. Sorts them, and takes the median index.
	/// 3. Removes any entries whose access index is ≤ that median.
	///
	/// **Note**: The chosen median-based strategy is a compromise. It tries to remove
	/// roughly half the entries (the older ones) at once, thereby avoiding multiple small
	/// evictions. However, it’s not strictly LRU in a typical “remove one oldest item”
	/// sense. If you want a more standard LRU, consider a different data structure or
	/// approach (like `hash_linked::LRUCache`).
	fn cleanup(&mut self) {
		let mut indices: Vec<u64> = self.cache.values().map(|(_, i)| *i).collect();
		indices.sort_unstable();
		let median_index = indices[indices.len().div(2)];

		// Retain only those whose access index is greater than the median
		self.cache.retain(|_, (_, idx)| {
			if *idx <= median_index {
				false
			} else {
				*idx = 0; // Not strictly necessary, but can reset for clarity
				true
			}
		});
	}
}

impl<K, V> Debug for LimitedCache<K, V> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("LimitedCache")
			.field("length", &self.cache.len())
			.field("max_length", &self.max_length)
			.field("last_index", &self.last_index)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::LimitedCache;
	use anyhow::{Result, anyhow};
	use std::mem::size_of;

	/// Ensures that creation with a given `maximum_size` sets the derived max_length appropriately.
	#[test]
	fn test_cache_initialization() {
		// Each (u64, i32) pair consumes size_of::<u64>() + size_of::<i32>() bytes.
		let element_size = size_of::<u64>() + size_of::<i32>();
		// Suppose we allow 100 bytes.
		let maximum_size = 100;
		let cache: LimitedCache<u64, i32> = LimitedCache::with_maximum_size(maximum_size);
		let expected_max_len = maximum_size / element_size;
		assert_eq!(cache.max_length, expected_max_len);
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

	/// Verifies that the internal cleanup is triggered once the cache hits capacity,
	/// and older items are evicted.
	#[test]
	fn test_capacity_and_cleanup() {
		let test = |max: u64, result: &[u64]| {
			let mut cache: LimitedCache<u64, u64> = LimitedCache::with_maximum_size(10 * (std::mem::size_of::<u64>()));
			for i in 0..=max {
				cache.add(i, i * 100);
				cache.get(&i);
			}
			let mut list: Vec<u64> = Vec::new();
			for i in 0..=9 {
				list.push(if cache.get(&i).is_some() { 1 } else { 0 });
			}
			assert_eq!(list.as_slice(), result, "error for test index {max}");
		};

		test(0, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
		test(1, &[1, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
		test(2, &[1, 1, 1, 0, 0, 0, 0, 0, 0, 0]);
		test(3, &[1, 1, 1, 1, 0, 0, 0, 0, 0, 0]);
		test(4, &[1, 1, 1, 1, 1, 0, 0, 0, 0, 0]);
		test(5, &[0, 0, 0, 1, 1, 1, 0, 0, 0, 0]);
		test(6, &[0, 0, 0, 1, 1, 1, 1, 0, 0, 0]);
		test(7, &[0, 0, 0, 1, 1, 1, 1, 1, 0, 0]);
		test(8, &[0, 0, 0, 0, 0, 0, 1, 1, 1, 0]);
		test(9, &[0, 0, 0, 0, 0, 0, 1, 1, 1, 1]);
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
		// Example: "LimitedCache { length: 0, max_length: 5, last_index: 0 }"
		assert!(debug_str.contains("LimitedCache"));
		assert!(debug_str.contains("length"));
		assert!(debug_str.contains("max_length"));
		assert!(debug_str.contains("last_index"));
	}
}
