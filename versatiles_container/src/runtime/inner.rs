use super::EventBus;
use crate::{CacheType, ContainerRegistry, ProgressFactory};
use std::{
	path::PathBuf,
	sync::{
		Mutex,
		atomic::{AtomicBool, AtomicUsize},
	},
};

pub struct RuntimeInner {
	pub cache_type: CacheType,
	pub ssh_identity: Option<PathBuf>,
	pub registry: ContainerRegistry,
	pub event_bus: EventBus,
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
		};

		// Verify event bus works
		inner.event_bus.step("Test step".to_string());
		// Event emitted successfully
	}
}
