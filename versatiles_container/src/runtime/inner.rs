use super::{EventBus, ProgressFactory};
use crate::{CacheType, ContainerRegistry};

pub struct RuntimeInner {
	pub cache_type: CacheType,
	pub registry: ContainerRegistry,
	pub event_bus: EventBus,
	pub progress_factory: ProgressFactory,
	pub max_memory: Option<usize>,
}
