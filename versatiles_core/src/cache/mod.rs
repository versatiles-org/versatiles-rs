mod cache_in_memory;
mod cache_on_disk;
mod map;
mod traits;

pub use map::CacheMap;
pub use traits::{Cache, CacheKey, CacheValue};
