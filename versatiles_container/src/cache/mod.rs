//! Caching subsystem for VersaTiles.
//!
//! This module provides flexible caching infrastructure used throughout the
//! VersaTiles processing and container layers. It supports both in-memory and
//! on-disk cache backends, allowing efficient reuse of intermediate computation
//! results such as decoded tiles, rendered images, or serialized metadata.
//!
//! # Submodules
//! - `cache_in_memory` — fast, non-persistent cache for small datasets
//! - `cache_on_disk` — disk-based cache storing data in binary files
//! - [`CacheType`] — defines which backend to use
//! - [`CacheMap`] — high-level cache wrapper for key→values storage
//! - [`Cache`], [`CacheKey`], [`CacheValue`] — core traits for cache key/value serialization
//!
//! # Usage
//! Cache type selection is determined by the [`TilesRuntime`](crate::TilesRuntime):
//!
//! The cache API provides consistent behavior across backends, with automatic
//! cleanup when dropped.

mod cache_in_memory;
mod cache_on_disk;
mod cache_type;
mod map;
mod traits;
mod traversal_cache;

pub use cache_type::CacheType;
pub use map::CacheMap;
pub use traits::{Cache, CacheKey, CacheValue};
pub use traversal_cache::TraversalCache;
