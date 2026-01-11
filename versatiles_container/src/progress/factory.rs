use crate::{EventBus, ProgressHandle};

/// Factory for creating progress bars
///
/// The factory maintains a global counter for unique progress IDs and creates
/// `ProgressHandle` instances that emit progress events through the event bus.
#[derive(Clone)]
pub struct ProgressFactory {
	next_id: u32,
	event_bus: EventBus,
	silent: bool,
}

impl ProgressFactory {
	/// Create a new progress factory
	#[must_use]
	pub fn new(event_bus: EventBus, silent: bool) -> Self {
		Self {
			next_id: 0,
			event_bus,
			silent,
		}
	}

	/// Create a new progress handle
	///
	/// # Arguments
	/// * `message` - Description of the operation being tracked
	/// * `total` - Total number of items/bytes to process
	/// * `event_bus` - Event bus to emit progress events to
	pub fn create(&mut self, message: &str, total: u64) -> ProgressHandle {
		self.next_id = self.next_id.wrapping_add(1);
		let id = crate::ProgressId(self.next_id);
		ProgressHandle::new(id, message.to_string(), total, self.event_bus.clone(), self.silent)
	}
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;

	#[test]
	fn test_factory_creation() {
		let event_bus = EventBus::new();
		let factory = ProgressFactory::new(event_bus, false);
		assert_eq!(factory.next_id, 0);
		assert!(!factory.silent);
	}

	#[test]
	fn test_factory_creation_with_silent() {
		let event_bus = EventBus::new();
		let factory = ProgressFactory::new(event_bus, true);
		assert_eq!(factory.next_id, 0);
		assert!(factory.silent);
	}

	#[test]
	fn test_factory_creates_progress_with_incrementing_ids() {
		let event_bus = EventBus::new();
		let mut factory = ProgressFactory::new(event_bus, true);

		let handle1 = factory.create("Task 1", 100);
		let handle2 = factory.create("Task 2", 200);
		let handle3 = factory.create("Task 3", 300);

		assert_eq!(handle1.id().0, 1);
		assert_eq!(handle2.id().0, 2);
		assert_eq!(handle3.id().0, 3);
	}

	#[test]
	fn test_factory_id_wrapping() {
		let event_bus = EventBus::new();
		let mut factory = ProgressFactory::new(event_bus, true);

		// Set next_id to max value
		factory.next_id = u32::MAX;

		let handle1 = factory.create("Wrap test", 100);
		assert_eq!(handle1.id().0, 0); // Should wrap to 0

		let handle2 = factory.create("After wrap", 100);
		assert_eq!(handle2.id().0, 1); // Should continue from 1
	}

	#[test]
	fn test_factory_clone() {
		let event_bus = EventBus::new();
		let mut factory1 = ProgressFactory::new(event_bus, true);

		// Create one handle to increment the ID
		let _handle1 = factory1.create("First", 100);
		assert_eq!(factory1.next_id, 1);

		// Clone the factory
		let mut factory2 = factory1.clone();

		// Both factories should have independent counters after clone
		let handle2 = factory2.create("Second", 100);
		assert_eq!(handle2.id().0, 2);
	}

	#[test]
	fn test_factory_multiple_handles() {
		let event_bus = EventBus::new();
		let mut factory = ProgressFactory::new(event_bus, true);

		let handles: Vec<_> = (0..10)
			.map(|i| factory.create(&format!("Task {i}"), i as u64 * 100))
			.collect();

		assert_eq!(handles.len(), 10);
		for (i, handle) in handles.iter().enumerate() {
			assert_eq!(handle.id().0, (i + 1) as u32);
		}
	}
}
