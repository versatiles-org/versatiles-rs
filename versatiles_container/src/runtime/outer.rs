use super::{EventBus, RuntimeBuilder, RuntimeInner};
use crate::{CacheType, DataSource, ProgressHandle, TilesReaderTrait};
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
	pub fn new() -> Self {
		Self::builder().build(true)
	}

	pub fn new_silent() -> Self {
		Self::builder().build(false)
	}

	/// Create a builder for customizing runtime configuration
	///
	/// # Examples
	///
	/// ```no_run
	/// use versatiles_container::TilesRuntime;
	///
	/// let runtime = TilesRuntime::builder()
	///     .with_disk_cache()
	///     .max_memory(2_000_000_000)
	///     .build(false);
	/// ```
	pub fn builder() -> RuntimeBuilder {
		RuntimeBuilder::default()
	}

	/// Get the cache type configuration
	pub fn cache_type(&self) -> &CacheType {
		&self.inner.cache_type
	}

	/// Get the event bus
	///
	/// Use the event bus to subscribe to runtime events or emit custom events.
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
	pub fn create_progress(&self, message: &str, total: u64) -> ProgressHandle {
		self.inner.progress_factory.lock().unwrap().create(message, total)
	}

	/// Get maximum memory limit (if configured)
	///
	/// Returns None if no memory limit was set, or Some(bytes) if a limit
	/// was configured during runtime creation.
	pub fn max_memory(&self) -> Option<usize> {
		self.inner.max_memory
	}

	pub async fn write_to_path(&self, reader: Box<dyn TilesReaderTrait>, path: &Path) -> Result<()> {
		self.inner.registry.write_to_path(reader, path, self.clone()).await
	}

	pub async fn get_reader_from_str(&self, filename: &str) -> Result<Box<dyn TilesReaderTrait>> {
		self.inner.registry.get_reader_from_str(filename, self.clone()).await
	}

	pub async fn get_reader(&self, data_source: DataSource) -> Result<Box<dyn TilesReaderTrait>> {
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
		assert!(runtime.max_memory().is_none());
	}

	#[test]
	fn test_runtime_builder() {
		let runtime = TilesRuntime::builder()
			.max_memory(1024)
			.with_memory_cache()
			.build(false);

		assert_eq!(runtime.max_memory(), Some(1024));
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
		// Initial + set_position + inc + finish = 4 events
		assert!(captured.len() >= 4);
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
