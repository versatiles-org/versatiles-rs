use std::path::PathBuf;
use uuid::Uuid;

lazy_static::lazy_static! {
	pub static ref DEFAULT_CACHE_DIR: PathBuf = std::env::var("VERSATILES_CACHE_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from).join(random_path());
}

#[derive(Clone, Debug)]
pub enum CacheType {
	InMemory,
	Disk(PathBuf), // path to cache directory
}

impl CacheType {
	#[must_use]
	pub fn new_disk() -> Self {
		Self::Disk(DEFAULT_CACHE_DIR.to_path_buf())
	}
	#[must_use]
	pub fn new_memory() -> Self {
		Self::InMemory
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
