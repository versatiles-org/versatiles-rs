use std::path::PathBuf;

pub enum CacheKind {
	InMemory,
	Disk(PathBuf), // path to cache directory
}

impl CacheKind {
	pub fn new_disk() -> Self {
		Self::Disk(std::env::temp_dir()) // default to system temp directory
	}
	pub fn new_memory() -> Self {
		Self::InMemory
	}
}
