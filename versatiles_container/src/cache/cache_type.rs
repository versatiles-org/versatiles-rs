//! Defines the cache type used by VersaTiles components.
//!
//! This module provides an enum [`CacheType`] that determines how and where
//! data is temporarily stored during processing — either in memory or on disk.
//!
//! The default disk cache directory can be controlled with the environment
//! variable `VERSATILES_CACHE_DIR`. If unset, a random directory is created
//! inside the system temporary folder.
use std::path::PathBuf;
use uuid::Uuid;

lazy_static::lazy_static! {
	pub static ref DEFAULT_CACHE_DIR: PathBuf = std::env::var("VERSATILES_CACHE_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from).join(random_path());
}

/// Defines how temporary data is cached during tile or dataset processing.
///
/// - `InMemory` — Keeps cache data entirely in memory. Fast but limited by available RAM.
/// - `Disk(PathBuf)` — Stores cache files on disk in the specified directory.
///
/// The choice depends on workload and resource constraints. Disk caching is
/// recommended for large datasets or multi-step pipelines.
#[derive(Clone, Debug)]
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
	pub fn new_disk() -> Self {
		Self::Disk(DEFAULT_CACHE_DIR.to_path_buf())
	}
	#[must_use]
	/// Create a [`CacheType::InMemory`] variant.
	///
	/// Use this for lightweight workloads that fit comfortably in RAM.
	pub fn new_memory() -> Self {
		Self::InMemory
	}
}

/// Generate a unique, timestamped random directory name for temporary cache usage.
///
/// The name format is:
/// `versatiles_YYYYMMDD_HHMMSS_XXXX`
/// where `XXXX` is a short random suffix derived from a UUID.
///
/// This ensures that multiple processes or test runs do not collide on cache directories.
fn random_path() -> String {
	use ::time::{OffsetDateTime, format_description::parse};
	let time = OffsetDateTime::now_local()
		.unwrap_or_else(|_| OffsetDateTime::now_utc())
		.format(&parse("[year][month][day]_[hour][minute][second]").unwrap())
		.unwrap();
	let mut rand = Uuid::new_v4().to_string();
	rand = rand.split_off(rand.len().saturating_sub(4));
	format!("versatiles_{time}_{rand}")
}

#[cfg(test)]
mod tests {
	use super::*;
	use wildmatch::WildMatch;

	#[test]
	fn test_random_path_format() {
		let path = random_path();
		assert!(
			WildMatch::new("versatiles_????????_??????_????").matches(&path),
			"random_path format is incorrect: {path}"
		);
	}

	#[test]
	fn test_random_path_uniqueness() {
		let path1 = random_path();
		let path2 = random_path();
		assert_ne!(path1, path2, "random_path should generate unique values");
	}
}
