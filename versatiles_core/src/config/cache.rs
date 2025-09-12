use std::path::PathBuf;
use uuid::Uuid;

lazy_static::lazy_static! {
	pub static ref DEFAULT_CACHE_DIR: PathBuf = std::env::var("VERSATILES_CACHE_DIR")
		.map(PathBuf::from)
		.unwrap_or_else(|_| std::env::temp_dir().into()).join(random_path());
}

pub enum CacheKind {
	InMemory,
	Disk(PathBuf), // path to cache directory
}

impl CacheKind {
	pub fn new_disk() -> Self {
		Self::Disk(DEFAULT_CACHE_DIR.to_path_buf())
	}
	pub fn new_memory() -> Self {
		Self::InMemory
	}
}

impl Default for CacheKind {
	fn default() -> Self {
		Self::new_memory()
	}
}

fn random_path() -> String {
	use ::time::{OffsetDateTime, format_description::parse};
	let time = OffsetDateTime::now_local()
		.unwrap_or_else(|_| OffsetDateTime::now_utc())
		.format(&parse("[year][month][day]_[hour][minute][second]").unwrap())
		.unwrap();
	let mut rand = Uuid::new_v4().to_string();
	rand = rand.split_off(rand.len().saturating_sub(4));
	return format!("versatiles_{time}_{rand}",);
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
			"random_path format is incorrect: {}",
			path
		);
	}

	#[test]
	fn test_random_path_uniqueness() {
		let path1 = random_path();
		let path2 = random_path();
		assert_ne!(path1, path2, "random_path should generate unique values");
	}
}
