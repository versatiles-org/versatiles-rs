use super::{EventBus, RuntimeBuilder, RuntimeInner};
use crate::{CacheType, DataSource, ProgressHandle, TileSource};
use anyhow::Result;
use std::{path::Path, sync::Arc};

/// Immutable runtime configuration and services for tile processing operations
///
/// `TilesRuntime` provides:
/// - Global processing parameters (memory limits, cache preferences)
/// - Container format registry (readers/writers)
/// - Unified event system (logs, progress, messages)
/// - Progress tracking factory
///
/// Once created, the runtime is immutable and cheap to clone (Arc-based).
/// Share it across async tasks, threads, and processing pipelines.
#[derive(Clone)]
pub struct TilesRuntime {
	pub(crate) inner: Arc<RuntimeInner>,
}

impl TilesRuntime {
	/// Create a new runtime with default settings
	///
	/// Equivalent to `TilesRuntime::builder().build()`
	#[must_use]
	pub fn new() -> Self {
		Self::builder().build()
	}

	#[must_use]
	pub fn new_silent() -> Self {
		Self::builder().silent_progress(true).build()
	}

	/// Create a builder for customizing runtime configuration
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
	#[must_use]
	pub fn builder() -> RuntimeBuilder {
		RuntimeBuilder::default()
	}

	/// Get the cache type configuration
	#[must_use]
	pub fn cache_type(&self) -> &CacheType {
		&self.inner.cache_type
	}

	/// Get the event bus
	///
	/// Use the event bus to subscribe to runtime events or emit custom events.
	#[must_use]
	pub fn events(&self) -> &EventBus {
		&self.inner.event_bus
	}

	/// Create a progress bar for tracking operations
	///
	/// Progress bars emit events through the event bus and can be monitored
	/// by subscribing to Progress events.
	///
	/// # Arguments
	/// * `message` - Description of the operation
	/// * `total` - Total number of items/bytes to process
	///
	/// # Examples
	///
	/// ```no_run
	/// # use versatiles_container::TilesRuntime;
	/// let runtime = TilesRuntime::new();
	/// let progress = runtime.create_progress("Converting tiles", 1000);
	///
	/// for i in 0..1000 {
	///     progress.inc(1);
	/// }
	///
	/// progress.finish();
	/// ```
	#[must_use]
	pub fn create_progress(&self, message: &str, total: u64) -> ProgressHandle {
		self.inner.progress_factory.lock().unwrap().create(message, total)
	}

	pub async fn write_to_path(&self, reader: Arc<Box<dyn TileSource>>, path: &Path) -> Result<()> {
		self.inner.registry.write_to_path(reader, path, self.clone()).await
	}

	pub async fn get_reader_from_str(&self, filename: &str) -> Result<Arc<Box<dyn TileSource>>> {
		self.inner.registry.get_reader_from_str(filename, self.clone()).await
	}

	pub async fn get_reader(&self, data_source: DataSource) -> Result<Arc<Box<dyn TileSource>>> {
		self.inner.registry.get_reader(data_source, self.clone()).await
	}
}

impl Default for TilesRuntime {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use crate::Event;

	use super::*;

	#[test]
	fn test_runtime_creation() {
		let runtime = TilesRuntime::new();
		assert_eq!(runtime.cache_type(), &CacheType::InMemory);
	}

	#[test]
	fn test_runtime_builder() {
		let runtime = TilesRuntime::builder()
			.with_memory_cache()
			.silent_progress(true)
			.build();

		assert_eq!(runtime.cache_type(), &CacheType::InMemory);
	}

	#[test]
	fn test_event_bus() {
		let runtime = TilesRuntime::new();
		let events = Arc::new(std::sync::Mutex::new(Vec::new()));
		let events_clone = events.clone();

		runtime.events().subscribe(move |event| {
			events_clone.lock().unwrap().push(format!("{:?}", event));
		});

		runtime.events().step("Test step".to_string());
		runtime.events().warn("Test warning".to_string());
		runtime.events().error("Test error".to_string());

		let captured = events.lock().unwrap();
		assert_eq!(captured.len(), 3);
		assert!(captured[0].contains("Step"));
		assert!(captured[1].contains("Warning"));
		assert!(captured[2].contains("Error"));
	}

	#[test]
	fn test_progress_handle() {
		let runtime = TilesRuntime::new();
		let events = Arc::new(std::sync::Mutex::new(Vec::new()));
		let events_clone = events.clone();

		runtime.events().subscribe(move |event| {
			if matches!(event, Event::Progress { .. }) {
				events_clone.lock().unwrap().push(());
			}
		});

		let progress = runtime.create_progress("Test", 100);
		progress.set_position(50);
		progress.inc(25);
		progress.finish();

		let captured = events.lock().unwrap();
		// With throttling (10/sec), rapid updates are filtered out.
		// We expect at least: initial event + finish event = 2 events
		assert!(captured.len() >= 2);
	}

	#[test]
	fn test_runtime_clone() {
		let runtime = TilesRuntime::new();
		let runtime2 = runtime.clone();

		// Both should share the same event bus
		let events = Arc::new(std::sync::Mutex::new(Vec::new()));
		let events_clone = events.clone();

		runtime.events().subscribe(move |_event| {
			events_clone.lock().unwrap().push(());
		});

		runtime2.events().step("Test".to_string());

		let captured = events.lock().unwrap();
		assert_eq!(captured.len(), 1);
	}
}
