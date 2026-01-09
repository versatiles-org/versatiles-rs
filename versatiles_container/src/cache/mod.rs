//! Caching subsystem for `VersaTiles`.
//!
//! This module provides caching infrastructure for tile traversal operations.
//! It supports both in-memory and on-disk cache backends via [`CacheType`].
//!
//! # Components
//! - [`CacheType`] — runtime selection of cache backend (memory or disk)
//! - [`TraversalCache`] — specialized cache for tile traversal reordering
//! - [`CacheValue`] — trait for serializing values to disk cache
//!
//! The cache API provides consistent behavior across backends, with automatic
//! cleanup when dropped.

mod cache_type;
mod traits;
mod traversal_cache;

pub use cache_type::CacheType;
pub use traits::CacheValue;
pub use traversal_cache::TraversalCache;
