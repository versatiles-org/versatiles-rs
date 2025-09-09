use std::path::PathBuf;

pub enum CacheKind {
	InMemory,
	Disk(PathBuf), // path to cache directory
}

impl Default for CacheKind {
	fn default() -> Self {
		Self::InMemory
	}
}
