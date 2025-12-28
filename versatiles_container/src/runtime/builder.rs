//! Builder pattern for constructing TilesRuntime instances

use super::{EventBus, RuntimeInner, TilesRuntime};
use crate::{CacheType, ContainerRegistry, ProgressFactory};
use std::sync::{Arc, Mutex};

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
///     .silent()
///     .build();
/// ```
pub struct RuntimeBuilder {
	cache_type: Option<CacheType>,
	max_memory: Option<usize>,
	#[allow(clippy::type_complexity)]
	registry_customizer: Vec<Box<dyn FnOnce(&mut ContainerRegistry)>>,
	silent: bool,
}

impl RuntimeBuilder {
	/// Create a new runtime builder with default settings
	pub fn new() -> Self {
		Self {
			cache_type: None,
			max_memory: None,
			registry_customizer: Vec::new(),
			silent: false,
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

	pub fn silent(mut self) -> Self {
		self.silent = true;
		self
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
	///     .silent()
	///     .build();
	/// ```
	pub fn customize_registry<F>(mut self, customizer: F) -> Self
	where
		F: Fn(&mut ContainerRegistry) + 'static,
	{
		self.registry_customizer.push(Box::new(customizer));
		self
	}

	/// Build the runtime
	///
	/// Creates a new TilesRuntime with the configured settings.
	pub fn build(self) -> TilesRuntime {
		let cache_type = self.cache_type.unwrap_or_else(CacheType::new_memory);
		let event_bus = EventBus::new();

		let progress_factory = Mutex::new(ProgressFactory::new(event_bus.clone(), self.silent));

		// Create registry with default format handlers
		let mut registry = ContainerRegistry::default();

		// Apply customizations if provided
		for customizer in self.registry_customizer {
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_runtime_builder_new() {
		let builder = RuntimeBuilder::new();
		assert!(builder.cache_type.is_none());
		assert!(builder.max_memory.is_none());
		assert!(!builder.silent);
		assert_eq!(builder.registry_customizer.len(), 0);
	}

	#[test]
	fn test_runtime_builder_default() {
		let builder = RuntimeBuilder::default();
		assert!(builder.cache_type.is_none());
		assert!(builder.max_memory.is_none());
		assert!(!builder.silent);
	}

	#[test]
	fn test_runtime_builder_with_memory_cache() {
		let builder = RuntimeBuilder::new().with_memory_cache();
		assert!(builder.cache_type.is_some());
	}

	#[test]
	fn test_runtime_builder_with_disk_cache() {
		let builder = RuntimeBuilder::new().with_disk_cache();
		assert!(builder.cache_type.is_some());
	}

	#[test]
	fn test_runtime_builder_max_memory() {
		let builder = RuntimeBuilder::new().max_memory(1024 * 1024);
		assert_eq!(builder.max_memory, Some(1024 * 1024));
	}

	#[test]
	fn test_runtime_builder_silent() {
		let builder = RuntimeBuilder::new().silent();
		assert!(builder.silent);
	}

	#[test]
	fn test_runtime_builder_customize_registry() {
		let builder = RuntimeBuilder::new().customize_registry(|_registry| {
			// Custom registry modification
		});
		assert_eq!(builder.registry_customizer.len(), 1);
	}

	#[test]
	fn test_runtime_builder_multiple_customizers() {
		let builder = RuntimeBuilder::new()
			.customize_registry(|_registry| {})
			.customize_registry(|_registry| {});
		assert_eq!(builder.registry_customizer.len(), 2);
	}

	#[test]
	fn test_runtime_builder_chaining() {
		let builder = RuntimeBuilder::new()
			.with_memory_cache()
			.max_memory(2048)
			.silent()
			.customize_registry(|_| {});

		assert!(builder.cache_type.is_some());
		assert_eq!(builder.max_memory, Some(2048));
		assert!(builder.silent);
		assert_eq!(builder.registry_customizer.len(), 1);
	}

	#[test]
	fn test_runtime_builder_build() {
		let runtime = RuntimeBuilder::new().build();
		// Runtime should be created successfully
		let _events = runtime.events();
	}

	#[test]
	fn test_runtime_builder_build_with_memory_cache() {
		let runtime = RuntimeBuilder::new().with_memory_cache().build();
		assert!(matches!(runtime.cache_type(), CacheType::InMemory));
	}

	#[test]
	fn test_runtime_builder_build_with_disk_cache() {
		let runtime = RuntimeBuilder::new().with_disk_cache().build();
		assert!(matches!(runtime.cache_type(), CacheType::Disk(_)));
	}

	#[test]
	fn test_runtime_builder_build_with_max_memory() {
		let runtime = RuntimeBuilder::new().max_memory(4096).build();
		assert_eq!(runtime.max_memory(), Some(4096));
	}

	#[test]
	fn test_runtime_builder_build_silent() {
		let runtime = RuntimeBuilder::new().silent().build();
		// Create a progress and verify it works
		let progress = runtime.create_progress("Test", 100);
		progress.inc(10);
		// Silent mode means no stderr output, but progress should still work
	}

	#[test]
	fn test_runtime_builder_build_default_cache() {
		let runtime = RuntimeBuilder::new().build();
		// Default should be memory cache
		assert!(matches!(runtime.cache_type(), CacheType::InMemory));
	}

	#[test]
	fn test_runtime_builder_build_with_customizer() {
		let builder = RuntimeBuilder::new().customize_registry(|_registry| {
			// This closure will be called during build
		});

		let _runtime = builder.build();
		// If we got here without panic, the customizer was applied
	}

	#[test]
	fn test_runtime_builder_full_configuration() {
		let runtime = RuntimeBuilder::new()
			.with_disk_cache()
			.max_memory(8 * 1024 * 1024)
			.silent()
			.customize_registry(|_| {})
			.build();

		assert!(matches!(runtime.cache_type(), CacheType::Disk(_)));
		assert_eq!(runtime.max_memory(), Some(8 * 1024 * 1024));
	}
}
