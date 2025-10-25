use crate::CacheType;
use std::sync::Arc;
use versatiles_core::TileCompression;

#[derive(Clone, Debug)]
pub struct WriterConfig {
	pub cache_type: CacheType,
	pub tile_compression: Option<TileCompression>,
}

impl WriterConfig {
	#[must_use]
	pub fn arc(self) -> Arc<Self> {
		Arc::new(self)
	}
}

impl Default for WriterConfig {
	fn default() -> Self {
		Self {
			cache_type: CacheType::new_memory(),
			tile_compression: None,
		}
	}
}
