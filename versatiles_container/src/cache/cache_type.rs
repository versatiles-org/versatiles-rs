//! Defines the cache type used by `VersaTiles` components.
//!
//! This module provides an enum [`CacheType`] that determines how and where
//! data is temporarily stored during processing — either in memory or on disk.
//!
//! The default disk cache directory can be controlled with the environment
//! variable `VERSATILES_CACHE_DIR`. If unset, a random directory is created
//! inside the system temporary folder.
use std::path::PathBuf;

/// Defines how temporary data is cached during tile or dataset processing.
///
/// - `InMemory` — Keeps cache data entirely in memory. Fast but limited by available RAM.
/// - `Disk(PathBuf)` — Stores cache files on disk in the specified directory.
///
/// The choice depends on workload and resource constraints. Disk caching is
/// recommended for large datasets or multi-step pipelines.
#[derive(Clone, Debug, PartialEq)]
pub enum CacheType {
	/// Store cache data in memory.
	InMemory,
	/// Store cache data on disk at the specified path.
	Disk(PathBuf),
}

/// Constructors and helpers for creating and working with [`CacheType`] variants.
impl CacheType {
	#[must_use]
	/// Create a [`CacheType::Disk`] variant using the default cache directory.
	///
	/// The directory path is generated from `VERSATILES_CACHE_DIR` or, if unset,
	/// a new random path inside the system’s temporary directory.
	pub fn new_disk(path_buf: PathBuf) -> Self {
		Self::Disk(path_buf)
	}
	#[must_use]
	/// Create a [`CacheType::InMemory`] variant.
	///
	/// Use this for lightweight workloads that fit comfortably in RAM.
	pub fn new_memory() -> Self {
		Self::InMemory
	}
}
