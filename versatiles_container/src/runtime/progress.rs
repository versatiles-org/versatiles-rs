//! Progress tracking for runtime operations
//!
//! Provides a factory pattern for creating multiple independent progress bars
//! that emit events through the event bus.

use super::events::{EventBus, ProgressData, ProgressId};
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicU64, Ordering},
};
use std::time::Instant;

/// Factory for creating progress bars
///
/// The factory maintains a global counter for unique progress IDs and creates
/// `ProgressHandle` instances that emit progress events through the event bus.
#[derive(Clone)]
pub struct ProgressFactory {
	next_id: Arc<AtomicU64>,
}

impl ProgressFactory {
	/// Create a new progress factory
	pub fn new() -> Self {
		Self {
			next_id: Arc::new(AtomicU64::new(0)),
		}
	}

	/// Create a new progress handle
	///
	/// # Arguments
	/// * `message` - Description of the operation being tracked
	/// * `total` - Total number of items/bytes to process
	/// * `event_bus` - Event bus to emit progress events to
	pub fn create(&self, message: &str, total: u64, event_bus: &EventBus) -> ProgressHandle {
		let id = ProgressId(self.next_id.fetch_add(1, Ordering::SeqCst));
		ProgressHandle::new(id, message.to_string(), total, event_bus.clone())
	}
}

impl Default for ProgressFactory {
	fn default() -> Self {
		Self::new()
	}
}

/// Handle for tracking progress of an operation
///
/// Progress handles can be cloned and shared across threads. All clones
/// share the same underlying state and emit events to the same event bus.
#[derive(Clone)]
pub struct ProgressHandle {
	id: ProgressId,
	state: Arc<Mutex<ProgressState>>,
	event_bus: EventBus,
}

struct ProgressState {
	message: String,
	position: u64,
	total: u64,
	start: Instant,
	finished: bool,
}

impl ProgressHandle {
	fn new(id: ProgressId, message: String, total: u64, event_bus: EventBus) -> Self {
		let handle = Self {
			id,
			state: Arc::new(Mutex::new(ProgressState {
				message,
				position: 0,
				total,
				start: Instant::now(),
				finished: false,
			})),
			event_bus,
		};

		// Emit initial progress event
		handle.emit_update();
		handle
	}

	/// Set absolute position
	///
	/// The position will be clamped to the maximum value (total).
	pub fn set_position(&self, position: u64) {
		let mut state = self.state.lock().unwrap();
		state.position = position.min(state.total);
		drop(state);
		self.emit_update();
	}

	/// Increment position by delta
	///
	/// The position will be clamped to the maximum value (total).
	pub fn inc(&self, delta: u64) {
		let mut state = self.state.lock().unwrap();
		state.position = state.position.saturating_add(delta).min(state.total);
		drop(state);
		self.emit_update();
	}

	/// Set maximum value (total)
	///
	/// If the current position exceeds the new total, it will be clamped.
	pub fn set_max_value(&self, total: u64) {
		let mut state = self.state.lock().unwrap();
		state.total = total;
		if state.position > state.total {
			state.position = state.total;
		}
		drop(state);
		self.emit_update();
	}

	/// Mark progress as finished
	///
	/// Sets position to total and marks the progress as complete.
	pub fn finish(&self) {
		let mut state = self.state.lock().unwrap();
		state.position = state.total;
		state.finished = true;
		drop(state);
		self.emit_update();
	}

	/// Get the progress ID
	pub fn id(&self) -> ProgressId {
		self.id
	}

	/// Emit a progress update event
	fn emit_update(&self) {
		let state = self.state.lock().unwrap();

		let elapsed = state.start.elapsed();
		let elapsed_secs = elapsed.as_secs_f64();

		// Calculate speed (items/second)
		let speed = if elapsed_secs > 0.0 {
			state.position as f64 / elapsed_secs
		} else {
			0.0
		};

		// Calculate ETA (estimated time remaining in seconds)
		let eta = if state.position > 0 && state.position < state.total {
			elapsed_secs * ((state.total - state.position) as f64 / state.position as f64).max(0.0)
		} else {
			0.0
		};

		// Calculate percentage
		let percentage = if state.total > 0 {
			(state.position as f64 * 100.0 / state.total as f64).min(100.0)
		} else {
			0.0
		};

		let data = ProgressData {
			position: state.position,
			total: state.total,
			percentage,
			speed,
			eta,
			message: state.message.clone(),
		};

		drop(state);

		self.event_bus.progress(self.id, data);
	}
}
