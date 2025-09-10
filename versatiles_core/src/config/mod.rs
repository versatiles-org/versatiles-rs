pub use crate::config::cache::CacheKind;
mod cache;

pub struct Config {
	pub cache: CacheKind,
}

#[allow(clippy::derivable_impls)]
impl Default for Config {
	fn default() -> Self {
		Self {
			cache: CacheKind::new_memory(),
		}
	}
}
