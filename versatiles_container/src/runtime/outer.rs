use std::{
	collections::BTreeMap,
	path::Path,
	sync::{Arc, atomic::Ordering},
};

use anyhow::{Result, bail};

use super::{EventBus, RuntimeBuilder, RuntimeInner};
use crate::{CacheType, DataLocation, DataSource, ProgressHandle, SharedTileSource};

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

/// Reports the settings a reader's behaviour actually depends on, and nothing
/// about the shared machinery (registry, event bus, progress) that every
/// runtime carries alike.
///
/// Exists so the many sources that hold a runtime can keep `#[derive(Debug)]`
/// rather than each hand-writing an impl to skip this one field.
impl std::fmt::Debug for TilesRuntime {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TilesRuntime")
			.field("abort_on_error", &self.inner.abort_on_error.load(Ordering::Relaxed))
			.field("error_count", &self.error_count())
			.field("writer_options", &self.writer_options())
			.finish_non_exhaustive()
	}
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

	/// A format-specific writer option, if set.
	///
	/// See [`RuntimeBuilder::writer_option`](super::RuntimeBuilder::writer_option).
	#[must_use]
	pub fn writer_option(&self, key: &str) -> Option<String> {
		self
			.inner
			.writer_options
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.get(key)
			.cloned()
	}

	/// A writer option parsed as a boolean.
	///
	/// Accepts `true`/`false`, `1`/`0` and `yes`/`no`, case-insensitively.
	/// Anything else is an error rather than a silent `false` — a writer option
	/// that quietly does nothing is the failure this mechanism exists to avoid.
	///
	/// # Errors
	/// Returns an error if the value is set but is not one of the accepted forms.
	pub fn writer_option_bool(&self, key: &str) -> Result<Option<bool>> {
		let Some(value) = self.writer_option(key) else {
			return Ok(None);
		};
		match value.trim().to_ascii_lowercase().as_str() {
			"true" | "1" | "yes" => Ok(Some(true)),
			"false" | "0" | "no" => Ok(Some(false)),
			other => bail!("writer option '{key}' expects true or false, got '{other}'"),
		}
	}

	/// All writer options currently set, in key order.
	#[must_use]
	pub fn writer_options(&self) -> BTreeMap<String, String> {
		self
			.inner
			.writer_options
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.clone()
	}

	/// Replace the writer options after construction.
	///
	/// The CLI builds one shared runtime in `main` and dispatches it to every
	/// subcommand, so `convert` fills these in on entry — the same reason
	/// [`set_abort_on_error`](Self::set_abort_on_error) exists.
	pub fn set_writer_options(&self, options: BTreeMap<String, String>) {
		*self
			.inner
			.writer_options
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner) = options;
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

	/// Open a tile container reader, authenticating SFTP with `ssh_identity`.
	///
	/// The identity applies to this source alone and wins over
	/// [`Self::ssh_identity`], which applies to every source in the process.
	pub async fn reader_from_location_with_ssh_identity(
		&self,
		location: DataLocation,
		ssh_identity: Option<&Path>,
	) -> Result<SharedTileSource> {
		self
			.inner
			.registry
			.reader_from_location_with_ssh_identity(location, self.clone(), ssh_identity)
			.await
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
	use super::*;
	use crate::Event;

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
	fn writer_options_round_trip_through_builder_and_setter() {
		let runtime = TilesRuntime::builder()
			.writer_option("allow_unclustered", "true")
			.writer_option("temp_dir", "/tmp")
			.build();
		assert_eq!(runtime.writer_option("allow_unclustered").as_deref(), Some("true"));
		assert_eq!(runtime.writer_option("temp_dir").as_deref(), Some("/tmp"));
		assert_eq!(runtime.writer_option("missing"), None);
		assert_eq!(runtime.writer_options().len(), 2);

		// The CLI builds one runtime in main and fills these in per subcommand.
		runtime.set_writer_options(BTreeMap::from([("reorder".to_string(), "true".to_string())]));
		assert_eq!(runtime.writer_options().len(), 1);
		assert_eq!(runtime.writer_option("reorder").as_deref(), Some("true"));
		assert_eq!(runtime.writer_option("allow_unclustered"), None);
	}

	#[test]
	fn writer_option_bool_accepts_the_usual_spellings() {
		for value in ["true", "TRUE", " true ", "1", "yes"] {
			let runtime = TilesRuntime::builder().writer_option("k", value).build();
			assert_eq!(runtime.writer_option_bool("k").unwrap(), Some(true), "value {value:?}");
		}
		for value in ["false", "False", "0", "no"] {
			let runtime = TilesRuntime::builder().writer_option("k", value).build();
			assert_eq!(runtime.writer_option_bool("k").unwrap(), Some(false), "value {value:?}");
		}
	}

	#[test]
	fn writer_option_bool_rejects_anything_else() {
		// A value that quietly becomes `false` is the silent no-op this whole
		// mechanism exists to prevent.
		let runtime = TilesRuntime::builder().writer_option("k", "maybe").build();
		let err = runtime.writer_option_bool("k").unwrap_err().to_string();
		assert!(err.contains("expects true or false"), "unexpected: {err}");
		assert!(err.contains("maybe"), "the message should quote the value: {err}");

		assert_eq!(TilesRuntime::new().writer_option_bool("k").unwrap(), None);
	}

	#[test]
	fn writer_options_are_shared_between_clones() {
		let runtime = TilesRuntime::new();
		let clone = runtime.clone();
		runtime.set_writer_options(BTreeMap::from([("a".to_string(), "1".to_string())]));
		assert_eq!(clone.writer_option("a").as_deref(), Some("1"));
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
