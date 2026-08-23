use std::{
	collections::BTreeMap,
	path::PathBuf,
	sync::{
		Mutex,
		atomic::{AtomicBool, AtomicUsize},
	},
};

use super::EventBus;
use crate::{CacheType, ContainerRegistry, ProgressFactory};

/// The state a [`TilesRuntime`](crate::TilesRuntime) shares between every
/// operation running under it.
///
/// Held behind an `Arc` by the runtime handle, so cloning a runtime is cheap and
/// all clones see the same caches, registry and event bus.
pub struct RuntimeInner {
	/// Where operations should spill intermediate data.
	pub cache_type: CacheType,
	/// Private key file offered to SFTP sources, if the caller configured one.
	pub ssh_identity: Option<PathBuf>,
	/// The container formats this runtime can open and write.
	pub registry: ContainerRegistry,
	/// Where log, progress and error events are published.
	pub event_bus: EventBus,
	/// Hands out the progress bars for operations under this runtime.
	pub progress_factory: Mutex<ProgressFactory>,
	/// When `true`, operations running under this runtime should abort once
	/// their stream drains if any error has been recorded via
	/// `TilesRuntime::record_error`. Set by the `convert` entry points; left
	/// `false` for servers, which should log errors but keep running.
	///
	/// Atomic so the setting can be flipped after the runtime is built
	/// (e.g. per subcommand in the CLI), without requiring the caller to
	/// rebuild shared state like the registry and event bus.
	pub abort_on_error: AtomicBool,
	/// Count of errors recorded via `TilesRuntime::record_error`. Producers
	/// can inspect this after a stream drains to detect silent drops.
	pub error_count: AtomicUsize,
	/// Format-specific options for whichever writer this runtime ends up
	/// driving, e.g. `allow_unclustered=true` for PMTiles.
	///
	/// Writers receive no arguments of their own — `TilesWriter`'s methods are
	/// static and `TilesConverterParameters` is consumed by the reader wrapper —
	/// so the runtime is the only channel that reaches them.
	///
	/// `BTreeMap` rather than `HashMap` so error messages listing the options
	/// are in a stable order. Behind a `Mutex` for the same reason
	/// `abort_on_error` is atomic: the CLI builds one runtime in `main` and
	/// fills this in per subcommand.
	pub writer_options: Mutex<BTreeMap<String, String>>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_runtime_inner_construction() {
		let cache_type = CacheType::new_memory();
		let registry = ContainerRegistry::default();
		let event_bus = EventBus::new();
		let progress_factory = Mutex::new(ProgressFactory::new(event_bus.clone(), false));

		let inner = RuntimeInner {
			cache_type,
			ssh_identity: None,
			registry,
			event_bus,
			progress_factory,
			abort_on_error: AtomicBool::new(false),
			error_count: AtomicUsize::new(0),
			writer_options: Mutex::new(BTreeMap::new()),
		};

		assert_eq!(inner.cache_type, CacheType::InMemory);
	}

	#[test]
	fn test_runtime_inner_with_disk_cache() {
		let path_buf = std::path::PathBuf::from("/tmp/versatiles_cache");
		let cache_type = CacheType::new_disk(path_buf.clone());
		let registry = ContainerRegistry::default();
		let event_bus = EventBus::new();
		let progress_factory = Mutex::new(ProgressFactory::new(event_bus.clone(), false));

		let inner = RuntimeInner {
			cache_type,
			ssh_identity: None,
			registry,
			event_bus,
			progress_factory,
			abort_on_error: AtomicBool::new(false),
			error_count: AtomicUsize::new(0),
			writer_options: Mutex::new(BTreeMap::new()),
		};

		assert_eq!(inner.cache_type, CacheType::Disk(path_buf));
	}

	#[test]
	fn test_runtime_inner_progress_factory_mutex() {
		let cache_type = CacheType::new_memory();
		let registry = ContainerRegistry::default();
		let event_bus = EventBus::new();
		let progress_factory = Mutex::new(ProgressFactory::new(event_bus.clone(), false));

		let inner = RuntimeInner {
			cache_type,
			ssh_identity: None,
			registry,
			event_bus,
			progress_factory,
			abort_on_error: AtomicBool::new(false),
			error_count: AtomicUsize::new(0),
			writer_options: Mutex::new(BTreeMap::new()),
		};

		// Verify we can lock and access the progress factory
		let mut factory = inner.progress_factory.lock().unwrap();
		let _progress = factory.create("Test", 100);
		// Successfully created progress
	}

	#[test]
	fn test_runtime_inner_event_bus_access() {
		let cache_type = CacheType::new_memory();
		let registry = ContainerRegistry::default();
		let event_bus = EventBus::new();
		let progress_factory = Mutex::new(ProgressFactory::new(event_bus.clone(), false));

		let inner = RuntimeInner {
			cache_type,
			ssh_identity: None,
			registry,
			event_bus: event_bus.clone(),
			progress_factory,
			abort_on_error: AtomicBool::new(false),
			error_count: AtomicUsize::new(0),
			writer_options: Mutex::new(BTreeMap::new()),
		};

		// Verify event bus works
		inner.event_bus.step("Test step".to_string());
		// Event emitted successfully
	}
}
