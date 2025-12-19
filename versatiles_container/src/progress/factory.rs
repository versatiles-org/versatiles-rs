use crate::{EventBus, ProgressHandle};

/// Factory for creating progress bars
///
/// The factory maintains a global counter for unique progress IDs and creates
/// `ProgressHandle` instances that emit progress events through the event bus.
#[derive(Clone)]
pub struct ProgressFactory {
	next_id: u32,
	event_bus: EventBus,
	stderr: bool,
}

impl ProgressFactory {
	/// Create a new progress factory
	pub fn new(event_bus: EventBus, stderr: bool) -> Self {
		Self {
			next_id: 0,
			event_bus,
			stderr,
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
		ProgressHandle::new(id, message.to_string(), total, self.event_bus.clone(), self.stderr)
	}
}
