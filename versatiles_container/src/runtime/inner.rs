use super::{EventBus, ProgressFactory};
use crate::{CacheType, ContainerRegistry};

pub(crate) struct RuntimeInner {
	pub(crate) cache_type: CacheType,
	pub(crate) registry: ContainerRegistry,
	pub(crate) event_bus: EventBus,
	pub(crate) progress_factory: ProgressFactory,
	pub(crate) max_memory: Option<usize>,
}
