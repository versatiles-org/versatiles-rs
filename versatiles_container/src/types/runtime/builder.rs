//! Builder pattern for constructing TilesRuntime instances

use super::{EventBus, ProgressFactory, RuntimeInner, TilesRuntime};
use crate::{CacheType, ContainerRegistry};
use std::sync::Arc;

/// Builder for creating customized TilesRuntime instances
///
/// # Examples
///
/// ```no_run
/// use versatiles_container::TilesRuntime;
///
/// let runtime = TilesRuntime::builder()
///     .with_disk_cache()
///     .max_memory(2 * 1024 * 1024 * 1024)
///     .build();
/// ```
pub struct RuntimeBuilder {
	cache_type: Option<CacheType>,
	max_memory: Option<usize>,
	#[allow(clippy::type_complexity)]
	registry_customizer: Option<Box<dyn FnOnce(&mut ContainerRegistry)>>,
}

impl RuntimeBuilder {
	/// Create a new runtime builder with default settings
	pub fn new() -> Self {
		Self {
			cache_type: None,
			max_memory: None,
			registry_customizer: None,
		}
	}

	/// Set cache type (InMemory or Disk)
	pub fn cache_type(mut self, cache_type: CacheType) -> Self {
		self.cache_type = Some(cache_type);
		self
	}

	/// Use in-memory cache (default)
	pub fn with_memory_cache(self) -> Self {
		self.cache_type(CacheType::new_memory())
	}

	/// Use disk cache
	pub fn with_disk_cache(self) -> Self {
		self.cache_type(CacheType::new_disk())
	}

	/// Set maximum memory limit in bytes
	///
	/// This is a hint to processing operations about memory constraints.
	/// Individual operations may or may not respect this limit.
	pub fn max_memory(mut self, bytes: usize) -> Self {
		self.max_memory = Some(bytes);
		self
	}

	/// Customize the container registry
	///
	/// The customizer function is called with a mutable reference to the
	/// registry after it's created, allowing you to register custom readers
	/// and writers.
	///
	/// # Examples
	///
	/// ```no_run
	/// use versatiles_container::TilesRuntime;
	///
	/// let runtime = TilesRuntime::builder()
	///     .customize_registry(|registry| {
	///         // Register custom format handlers
	///     })
	///     .build();
	/// ```
	pub fn customize_registry<F>(mut self, customizer: F) -> Self
	where
		F: FnOnce(&mut ContainerRegistry) + 'static,
	{
		self.registry_customizer = Some(Box::new(customizer));
		self
	}

	/// Build the runtime
	///
	/// Creates a new TilesRuntime with the configured settings.
	pub fn build(self) -> TilesRuntime {
		let cache_type = self.cache_type.unwrap_or_else(CacheType::new_memory);
		let event_bus = EventBus::new();
		let progress_factory = ProgressFactory::new();

		// Create registry with default format handlers
		let mut registry = ContainerRegistry::default();

		// Apply customizations if provided
		if let Some(customizer) = self.registry_customizer {
			customizer(&mut registry);
		}

		TilesRuntime {
			inner: Arc::new(RuntimeInner {
				cache_type,
				registry,
				event_bus,
				progress_factory,
				max_memory: self.max_memory,
			}),
		}
	}
}

impl Default for RuntimeBuilder {
	fn default() -> Self {
		Self::new()
	}
}
