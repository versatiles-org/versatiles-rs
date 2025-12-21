use super::EventBus;
use crate::{CacheType, ContainerRegistry, ProgressFactory};
use std::sync::Mutex;

pub struct RuntimeInner {
	pub cache_type: CacheType,
	pub registry: ContainerRegistry,
	pub event_bus: EventBus,
	pub progress_factory: Mutex<ProgressFactory>,
	pub max_memory: Option<usize>,
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
			max_memory: Some(1024),
		};

		assert!(matches!(inner.cache_type, CacheType::InMemory));
		assert_eq!(inner.max_memory, Some(1024));
	}

	#[test]
	fn test_runtime_inner_with_no_max_memory() {
		let cache_type = CacheType::new_memory();
		let registry = ContainerRegistry::default();
		let event_bus = EventBus::new();
		let progress_factory = Mutex::new(ProgressFactory::new(event_bus.clone(), false));

		let inner = RuntimeInner {
			cache_type,
			registry,
			event_bus,
			progress_factory,
			max_memory: None,
		};

		assert!(inner.max_memory.is_none());
	}

	#[test]
	fn test_runtime_inner_with_disk_cache() {
		let cache_type = CacheType::new_disk();
		let registry = ContainerRegistry::default();
		let event_bus = EventBus::new();
		let progress_factory = Mutex::new(ProgressFactory::new(event_bus.clone(), false));

		let inner = RuntimeInner {
			cache_type,
			registry,
			event_bus,
			progress_factory,
			max_memory: None,
		};

		assert!(matches!(inner.cache_type, CacheType::Disk(_)));
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
			max_memory: None,
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
			max_memory: None,
		};

		// Verify event bus works
		inner.event_bus.step("Test step".to_string());
		// Event emitted successfully
	}
}
