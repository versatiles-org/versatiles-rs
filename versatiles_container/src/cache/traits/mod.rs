// Common trait definitions for the VersaTiles caching subsystem.
//
// This module groups the traits used by both in-memory and on-disk caches:
// - [`Cache`](crate::cache::traits::cache::Cache): generic caching interface
// - [`CacheKey`](crate::cache::traits::key::CacheKey): defines how keys are represented
// - [`CacheValue`](crate::cache::traits::value::CacheValue): defines how values are serialized
//
// These traits form the foundation of the caching API, enabling flexible
// and interchangeable cache backends (e.g., [`InMemoryCache`](crate::cache::memory::InMemoryCache)
// or [`OnDiskCache`](crate::cache::disk::OnDiskCache)).
//
// Each trait can be implemented for custom types to extend the caching system.

mod cache;
mod key;
mod value;

pub use cache::*;
pub use key::*;
pub use value::*;
