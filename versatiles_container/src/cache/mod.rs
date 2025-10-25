mod cache_in_memory;
mod cache_on_disk;
mod cache_type;
mod map;
mod traits;

pub use cache_type::CacheType;
pub use map::CacheMap;
pub use traits::{Cache, CacheKey, CacheValue};
