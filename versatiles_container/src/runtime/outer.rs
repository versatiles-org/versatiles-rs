use super::{EventBus, RuntimeBuilder, RuntimeInner};
use crate::{CacheType, DataLocation, DataSource, ProgressHandle, SharedTileSource};
use anyhow::Result;
use std::{
	path::Path,
	sync::{Arc, atomic::Ordering},
};

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

	/// Get the SSH identity file path, if configured
	#[must_use]
	pub fn ssh_identity(&self) -> Option<&Path> {
		self.inner.ssh_identity.as_deref()
	}

	/// Get the event bus
	///
	/// Use the event bus to subscribe to runtime events or emit custom events.
	#[must_use]
	pub fn events(&self) -> &EventBus {
		&self.inner.event_bus
	}

	/// Record a read error that would otherwise be silently dropped.
	///
	/// Always logs via the `log` crate and emits an `Event::Error` on the bus.
	/// When the runtime was built with
	/// [`RuntimeBuilder::abort_on_error(true)`](super::RuntimeBuilder::abort_on_error),
	/// [`had_errors`](Self::had_errors) starts returning `true` as soon as the
	/// first error is recorded — so callers can abort after their stream drains
	/// rather than silently producing truncated output.
	///
	/// # Arguments
	/// * `source` — short label identifying the producer (e.g.
	///   `"versatiles block index"`, `"from_tilejson tile z/x/y"`).
	/// * `err` — the underlying error. Its full context chain is preserved in
	///   the emitted message (formatted with `{:#}`).
	pub fn record_error(&self, source: &str, err: &anyhow::Error) {
		let message = format!("{source}: {err:#}");
		log::error!("{message}");
		self.inner.event_bus.error(message);
		self.inner.error_count.fetch_add(1, Ordering::Relaxed);
	}

	/// Number of errors recorded via [`record_error`](Self::record_error)
	/// since this runtime was built. Clones share the same counter.
	#[must_use]
	pub fn error_count(&self) -> usize {
		self.inner.error_count.load(Ordering::Relaxed)
	}

	/// `true` when the runtime was built (or subsequently set) in
	/// abort-on-error mode and at least one error has been recorded.
	///
	/// Convert entry points use this after their stream drains to turn silent
	/// drops into hard failures. Servers leave abort-on-error disabled, so
	/// this always returns `false` for them.
	#[must_use]
	pub fn had_errors(&self) -> bool {
		self.inner.abort_on_error.load(Ordering::Relaxed) && self.error_count() > 0
	}

	/// Override the abort-on-error flag after construction.
	///
	/// The CLI builds one shared runtime in `main` and dispatches it to every
	/// subcommand. The `convert` subcommand flips this to `true` on entry so
	/// the conversion aborts on any silent-drop error; `serve` leaves it
	/// `false` so the server keeps running when a read fails.
	pub fn set_abort_on_error(&self, abort: bool) {
		self.inner.abort_on_error.store(abort, Ordering::Relaxed);
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
		self
			.inner
			.progress_factory
			.lock()
			.expect("poisoned mutex")
			.create(message, total)
	}

	pub async fn write_to_path(&self, reader: SharedTileSource, path: &Path) -> Result<()> {
		self.inner.registry.write_to_path(reader, path, self.clone()).await
	}

	/// Write tiles to a destination specified as a string (path or SFTP URL).
	pub async fn write_to_str(&self, reader: SharedTileSource, destination: &str) -> Result<()> {
		self
			.inner
			.registry
			.write_to_str(reader, destination, self.clone())
			.await
	}

	pub async fn reader_from_str(&self, filename: &str) -> Result<SharedTileSource> {
		self.inner.registry.reader_from_str(filename, self.clone()).await
	}

	/// Open a tile container reader from a [`DataLocation`] (path or URL).
	pub async fn reader_from_location(&self, location: DataLocation) -> Result<SharedTileSource> {
		self.inner.registry.reader_from_location(location, self.clone()).await
	}

	pub async fn reader(&self, data_source: DataSource) -> Result<SharedTileSource> {
		self.inner.registry.reader(data_source, self.clone()).await
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
			events_clone.lock().unwrap().push(format!("{event:?}"));
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

	#[test]
	fn record_error_default_runtime_never_flags_had_errors() {
		let runtime = TilesRuntime::new();
		assert!(!runtime.had_errors());
		runtime.record_error("test source", &anyhow::anyhow!("boom"));
		assert_eq!(runtime.error_count(), 1);
		// abort_on_error is false → had_errors stays false
		assert!(!runtime.had_errors());
	}

	#[test]
	fn record_error_abort_mode_flags_had_errors() {
		let runtime = TilesRuntime::builder().abort_on_error(true).build();
		assert!(!runtime.had_errors());
		assert_eq!(runtime.error_count(), 0);

		runtime.record_error("test source", &anyhow::anyhow!("boom"));

		assert_eq!(runtime.error_count(), 1);
		assert!(runtime.had_errors());
	}

	#[test]
	fn record_error_emits_event_with_context_chain() {
		let runtime = TilesRuntime::builder().abort_on_error(true).build();
		let messages = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
		let messages_clone = messages.clone();
		runtime.events().subscribe(move |event| {
			if let Event::Error { message } = event {
				messages_clone.lock().unwrap().push(message.clone());
			}
		});

		let err = anyhow::anyhow!("root cause").context("middle").context("outer");
		runtime.record_error("producer X", &err);

		let captured = messages.lock().unwrap();
		assert_eq!(captured.len(), 1);
		let msg = &captured[0];
		assert!(msg.contains("producer X"));
		// {:#} formatter walks the full chain
		assert!(msg.contains("outer"));
		assert!(msg.contains("middle"));
		assert!(msg.contains("root cause"));
	}

	#[test]
	fn set_abort_on_error_flips_flag_post_construction() {
		let runtime = TilesRuntime::new(); // builder default: abort_on_error = false
		runtime.record_error("src", &anyhow::anyhow!("a"));
		assert!(!runtime.had_errors(), "without abort_on_error the flag stays off");

		runtime.set_abort_on_error(true);
		// The same recorded error now flips had_errors().
		assert!(runtime.had_errors());

		runtime.set_abort_on_error(false);
		assert!(!runtime.had_errors());
	}

	#[test]
	fn record_error_counter_shared_across_clones() {
		let runtime = TilesRuntime::builder().abort_on_error(true).build();
		let clone = runtime.clone();

		runtime.record_error("src", &anyhow::anyhow!("a"));
		clone.record_error("src", &anyhow::anyhow!("b"));

		// Both views see the combined count.
		assert_eq!(runtime.error_count(), 2);
		assert_eq!(clone.error_count(), 2);
		assert!(runtime.had_errors());
		assert!(clone.had_errors());
	}
}
