//! Generic caching trait abstraction used by both in-memory and on-disk caches.
//!
//! The [`Cache`] trait defines the minimal interface for key→values caching mechanisms
//! used throughout VersaTiles. Implementations include:
//! - [`InMemoryCache`](crate::cache::memory::InMemoryCache)
//! - [`OnDiskCache`](crate::cache::disk::OnDiskCache)
//!
//! A cache associates a [`CacheKey`] with a list of [`CacheValue`] entries,
//! allowing multi-valued storage (a key can map to several values).
//!
//! Implementations are required to handle cleanup (e.g., removing temporary files)
//! in their [`clean_up`](Cache::clean_up) method.

use super::{CacheKey, CacheValue};
use anyhow::Result;
use std::fmt::Debug;

/// A trait defining the behavior of a generic key→values cache.
///
/// This abstraction allows different backend implementations (e.g. in-memory, on-disk)
/// to provide the same interface for storing and retrieving cached data.
///
/// # Type Parameters
/// * `K` — Cache key type implementing [`CacheKey`].
/// * `V` — Cache value type implementing [`CacheValue`].
pub trait Cache<K: CacheKey, V: CacheValue>: Debug {
	/// Return `true` if the cache contains an entry for the specified `key`.
	fn contains_key(&self, key: &K) -> bool;

	/// Retrieve a cloned list of values associated with `key`.
	///
	/// Returns:
	/// * `Ok(Some(values))` — if the key exists.
	/// * `Ok(None)` — if the key is not found.
	/// * `Err(_)` — if a backend error occurs.
	fn get_clone(&self, key: &K) -> Result<Option<Vec<V>>>;

	/// Remove and return the cached values for the given `key`, if they exist.
	///
	/// This operation may delete files or free memory depending on the backend.
	fn remove(&mut self, key: &K) -> Result<Option<Vec<V>>>;

	/// Insert or overwrite the list of values for a given `key`.
	///
	/// Replaces any previous values associated with the key.
	fn insert(&mut self, key: &K, values: Vec<V>) -> Result<()>;

	/// Append one or more values to the existing list for `key`.
	///
	/// Creates a new entry if the key does not yet exist.
	fn append(&mut self, key: &K, values: Vec<V>) -> Result<()>;

	/// Perform backend-specific cleanup, such as freeing memory or removing temporary files.
	///
	/// Called automatically by higher-level abstractions when caches are dropped.
	fn clean_up(&mut self);
}
