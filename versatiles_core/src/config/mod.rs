pub use crate::config::cache_type::CacheType;
use std::sync::Arc;
mod cache_type;

#[derive(Clone, Debug)]
pub struct Config {
	pub cache_type: CacheType,
}

impl Config {
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
