use crate::CacheType;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ProcessingConfig {
	pub cache_type: CacheType,
}

impl ProcessingConfig {
	#[must_use]
	pub fn arc(self) -> Arc<Self> {
		Arc::new(self)
	}
}

impl Default for ProcessingConfig {
	fn default() -> Self {
		Self {
			cache_type: CacheType::new_memory(),
		}
	}
}
