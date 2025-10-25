use crate::CacheType;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Config {
	pub cache_type: CacheType,
}

impl Config {
	#[must_use]
	pub fn arc(self) -> Arc<Self> {
		Arc::new(self)
	}
}

impl Default for Config {
	fn default() -> Self {
		Self {
			cache_type: CacheType::new_memory(),
		}
	}
}
