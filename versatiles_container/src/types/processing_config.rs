//! The `ProcessingConfig` struct defines runtime parameters for tile processing.
//!
//! It encapsulates configuration that affects how data is read, processed, and cached.
//! The most important field is `cache_type`, which controls whether an in-memory cache
//! or another cache backend is used by various data readers and writers.
//!
//! The configuration is usually cloned or wrapped in an [`Arc`](std::sync::Arc)
//! to share it safely between async tasks and threads.

use crate::CacheType;
use std::sync::Arc;
use versatiles_core::progress::ProgressBar;

/// Configuration parameters controlling data processing behavior.
///
/// Currently only the cache backend is configurable, but this struct is designed
/// to be extended with more runtime parameters (e.g., parallelism limits,
/// I/O buffer sizes, or tile transformation options).
///
/// Typical usage:
/// ```no_run
/// use versatiles_container::ProcessingConfig;
/// let config = ProcessingConfig::default();
/// let config_arc = config.arc();
/// ```
pub struct ProcessingConfig {
	/// The type of cache backend to use for tile data.
	pub cache_type: CacheType,
	/// Optional progress bar for monitoring tile processing progress.
	pub progress_bar: Option<ProgressBar>,
}

impl std::fmt::Debug for ProcessingConfig {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ProcessingConfig")
			.field("cache_type", &self.cache_type)
			.field("progress_bar", &self.progress_bar.as_ref().map(|_| "Some(ProgressBar)"))
			.finish()
	}
}

impl ProcessingConfig {
	/// Convert the configuration into an [`Arc`](std::sync::Arc),
	/// allowing safe shared access across threads and async tasks.
	///
	/// # Returns
	/// A new `Arc<ProcessingConfig>` containing this instance.
	#[must_use]
	pub fn arc(self) -> Arc<Self> {
		Arc::new(self)
	}
}

/// Provides a reasonable default configuration.
///
/// Uses an in-memory cache backend by default and no progress monitoring.
impl Default for ProcessingConfig {
	fn default() -> Self {
		Self {
			cache_type: CacheType::new_memory(),
			progress_bar: None,
		}
	}
}
