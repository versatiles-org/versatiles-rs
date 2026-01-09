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
///     .with_disk_cache(std::path::Path::new("/tmp/versatiles_cache"))
///     .silent_progress(true)
///     .build();
/// ```
pub struct RuntimeBuilder {
	cache_type: Option<CacheType>,
	#[allow(clippy::type_complexity)]
	registry_customizer: Vec<Box<dyn FnOnce(&mut ContainerRegistry)>>,
	silent_progress: bool,
}

impl RuntimeBuilder {
	/// Create a new runtime builder with default settings
	pub fn new() -> Self {
		Self {
			cache_type: None,
			registry_customizer: Vec::new(),
			#[cfg(not(test))]
			silent_progress: false,
			#[cfg(test)]
			silent_progress: true,
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
	pub fn with_disk_cache(self, path: &std::path::Path) -> Self {
		self.cache_type(CacheType::Disk(path.to_path_buf()))
	}

	pub fn silent_progress(mut self, silent: bool) -> Self {
		self.silent_progress = silent;
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
	///     .silent_progress(true)
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

		let progress_factory = Mutex::new(ProgressFactory::new(event_bus.clone(), self.silent_progress));

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
	use std::path::{Path, PathBuf};

	use super::*;

	#[test]
	fn test_runtime_builder_new() {
		let builder = RuntimeBuilder::new();
		assert!(builder.cache_type.is_none());
		assert!(builder.silent_progress);
		assert_eq!(builder.registry_customizer.len(), 0);
	}

	#[test]
	fn test_runtime_builder_default() {
		let builder = RuntimeBuilder::default();
		assert!(builder.cache_type.is_none());
		assert!(builder.silent_progress);
	}

	#[test]
	fn test_runtime_builder_with_memory_cache() {
		let builder = RuntimeBuilder::new().with_memory_cache();
		assert!(builder.cache_type.is_some());
	}

	#[test]
	fn test_runtime_builder_with_disk_cache() {
		let builder = RuntimeBuilder::new().with_disk_cache(Path::new("/tmp/cache"));
		assert!(builder.cache_type.is_some());
	}

	#[test]
	fn test_runtime_builder_silent() {
		let mut builder = RuntimeBuilder::new();
		builder = builder.silent_progress(true);
		assert!(builder.silent_progress);
		builder = builder.silent_progress(false);
		assert!(!builder.silent_progress);
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
			.silent_progress(true)
			.customize_registry(|_| {});

		assert!(builder.cache_type.is_some());
		assert!(builder.silent_progress);
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
		assert_eq!(runtime.cache_type(), &CacheType::InMemory);
	}

	#[test]
	fn test_runtime_builder_build_with_disk_cache() {
		let path_buf = PathBuf::from("/tmp/test_cache");
		let runtime = RuntimeBuilder::new().with_disk_cache(&path_buf).build();
		assert_eq!(runtime.cache_type(), &CacheType::Disk(path_buf));
	}

	#[test]
	fn test_runtime_builder_build_silent() {
		let runtime = RuntimeBuilder::new().silent_progress(true).build();
		// Create a progress and verify it works
		let progress = runtime.create_progress("Test", 100);
		progress.inc(10);
		// Silent mode means no stderr output, but progress should still work
	}

	#[test]
	fn test_runtime_builder_build_default_cache() {
		let runtime = RuntimeBuilder::new().build();
		// Default should be memory cache
		assert_eq!(runtime.cache_type(), &CacheType::InMemory);
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
		let path_buf = PathBuf::from("/tmp/full_config_cache");
		let runtime = RuntimeBuilder::new()
			.with_disk_cache(&path_buf)
			.silent_progress(true)
			.customize_registry(|_| {})
			.build();

		assert_eq!(runtime.cache_type(), &CacheType::Disk(path_buf));
	}
}
