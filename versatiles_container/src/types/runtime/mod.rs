//! Runtime configuration and services for tile processing operations
//!
//! The runtime provides a unified interface for:
//! - Global processing parameters (memory limits, cache configuration)
//! - Container format registry (readers/writers)
//! - Unified event bus (logs, progress, messages, warnings, errors)
//! - Progress bar factory (create multiple independent progress bars)
//!
//! # Example
//!
//! ```no_run
//! use versatiles_container::TilesRuntime;
//!
//! let runtime = TilesRuntime::builder()
//!     .with_memory_cache()
//!     .max_memory(2 * 1024 * 1024 * 1024)
//!     .build();
//!
//! // Subscribe to events
//! runtime.events().subscribe(|event| {
//!     println!("{:?}", event);
//! });
//!
//! // Create progress bars
//! let progress = runtime.create_progress("Processing", 1000);
//! progress.inc(100);
//! progress.finish();
//! ```

mod builder;
mod events;
mod inner;
mod progress;

pub use builder::RuntimeBuilder;
pub use events::{Event, EventBus, ListenerId, LogAdapter, LogLevel, ProgressData, ProgressId};
pub use progress::{ProgressFactory, ProgressHandle};

use crate::{CacheType, ContainerRegistry};
use inner::RuntimeInner;
use std::sync::Arc;

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
		Self::builder().build()
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
	///     .build();
	/// ```
	pub fn builder() -> RuntimeBuilder {
		RuntimeBuilder::default()
	}

	/// Get the cache type configuration
	pub fn cache_type(&self) -> &CacheType {
		&self.inner.cache_type
	}

	/// Get the container registry
	///
	/// The registry provides access to tile container readers and writers.
	pub fn registry(&self) -> &ContainerRegistry {
		&self.inner.registry
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
		self
			.inner
			.progress_factory
			.create(message, total, &self.inner.event_bus)
	}

	/// Get maximum memory limit (if configured)
	///
	/// Returns None if no memory limit was set, or Some(bytes) if a limit
	/// was configured during runtime creation.
	pub fn max_memory(&self) -> Option<usize> {
		self.inner.max_memory
	}
}

impl Default for TilesRuntime {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_runtime_creation() {
		let runtime = TilesRuntime::new();
		assert!(runtime.max_memory().is_none());
	}

	#[test]
	fn test_runtime_builder() {
		let runtime = TilesRuntime::builder().max_memory(1024).with_memory_cache().build();

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
