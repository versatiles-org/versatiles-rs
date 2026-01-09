use super::EventBus;
use crate::{CacheType, ContainerRegistry, ProgressFactory};
use std::sync::Mutex;

pub struct RuntimeInner {
	pub cache_type: CacheType,
	pub registry: ContainerRegistry,
	pub event_bus: EventBus,
	pub progress_factory: Mutex<ProgressFactory>,
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
			registry,
			event_bus,
			progress_factory,
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
			registry,
			event_bus,
			progress_factory,
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
			registry,
			event_bus,
			progress_factory,
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
			registry,
			event_bus: event_bus.clone(),
			progress_factory,
		};

		// Verify event bus works
		inner.event_bus.step("Test step".to_string());
		// Event emitted successfully
	}
}
