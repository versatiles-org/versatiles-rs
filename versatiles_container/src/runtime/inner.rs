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
