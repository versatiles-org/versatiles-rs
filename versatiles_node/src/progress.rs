use napi::{
	bindgen_prelude::*,
	threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;
use std::{
	sync::{Arc, Mutex},
	time::Instant,
};
use versatiles_container::ProgressState;

lazy_static::lazy_static! {
	static ref UNIX_EPOCH_INSTANT: Instant = Instant::now() - std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
}

/// Progress data sent to JavaScript callbacks
#[napi(object)]
#[derive(Clone)]
pub struct ProgressData {
	pub position: f64,
	pub total: f64,
	pub percentage: f64,
	pub speed: f64,
	pub estimated_seconds_remaining: f64,
	/// ETA in milliseconds since UNIX epoch (can be converted to Date with `new Date(eta)`)
	pub eta: f64,
	pub message: Option<String>,
}

impl From<&ProgressState> for ProgressData {
	fn from(data: &ProgressState) -> Self {
		let speed = data.position as f64 / data.start.elapsed().as_secs_f64();
		let estimated_seconds_remaining = if speed > 0.0 {
			(data.total as f64 - data.position as f64) / speed
		} else {
			f64::INFINITY
		};
		let eta = if estimated_seconds_remaining.is_finite() {
			// Return milliseconds since UNIX epoch for JavaScript Date
			(data.start.duration_since(*UNIX_EPOCH_INSTANT).as_secs_f64() + data.total as f64 / speed) * 1000.0
		} else {
			f64::INFINITY
		};
		ProgressData {
			estimated_seconds_remaining,
			eta,
			message: Some(data.message.clone()),
			percentage: data.position as f64 / data.total as f64 * 100.0,
			position: data.position as f64,
			speed,
			total: data.total as f64,
		}
	}
}

/// Message data sent to JavaScript callbacks
#[napi(object)]
#[derive(Clone)]
pub struct MessageData {
	#[napi(js_name = "type")]
	pub msg_type: String,
	pub message: String,
}

// Type aliases for the three different callback types
// Note: Using weak references (Weak=true) to avoid blocking process exit
type ProgressCallback = ThreadsafeFunction<ProgressData, Unknown<'static>, ProgressData, Status, false, true>;
type MessageCallback = ThreadsafeFunction<(String, String), Unknown<'static>, (String, String), Status, false, true>;

/// Progress monitor for long-running operations
///
/// This class allows monitoring the progress of tile conversion and other
/// long-running operations through event listeners.
#[napi]
#[derive(Clone)]
pub struct Progress {
	// Event listeners with typed callbacks
	progress_listeners: Arc<Mutex<Vec<ProgressCallback>>>,
	message_listeners: Arc<Mutex<Vec<MessageCallback>>>,
}

#[napi]
impl Progress {
	/// Create a new Progress instance
	pub fn new() -> Self {
		Progress {
			progress_listeners: Arc::new(Mutex::new(Vec::new())),
			message_listeners: Arc::new(Mutex::new(Vec::new())),
		}
	}
}

impl Default for Progress {
	fn default() -> Self {
		Self::new()
	}
}

#[napi]
impl Progress {
	/// Register a progress event listener
	///
	/// The callback receives ProgressData with position, total, percentage, speed, eta, and message.
	/// The eta field is a JavaScript Date object representing the estimated time of completion.
	#[napi(ts_args_type = "callback: (data: ProgressData) => void")]
	pub fn on_progress(&self, callback: Function<'static>) -> Result<&Self> {
		let tsfn = callback
			.build_threadsafe_function::<ProgressData>()
			.weak::<true>()
			.build_callback(|ctx| Ok(ctx.value))?;
		let mut listeners = self.progress_listeners.lock().unwrap();
		listeners.push(tsfn);
		Ok(self)
	}

	/// Register a message event listener for step, warning, and error messages
	///
	/// The callback receives (type, message) where type is 'step', 'warning', or 'error'
	#[napi(ts_args_type = "callback: (type: string, message: string) => void")]
	pub fn on_message(&self, callback: Function<'static>) -> Result<&Self> {
		let tsfn = callback
			.build_threadsafe_function::<(String, String)>()
			.weak::<true>()
			.build_callback(|ctx| Ok(ctx.value))?;
		let mut listeners = self.message_listeners.lock().unwrap();
		listeners.push(tsfn);
		Ok(self)
	}
}

impl Progress {
	/// Emit a progress event to all registered listeners
	pub fn emit_progress(&self, data: ProgressData) {
		let listeners = self.progress_listeners.lock().unwrap();
		for listener in listeners.iter() {
			let _ = listener.call(data.clone(), ThreadsafeFunctionCallMode::NonBlocking);
		}
	}

	/// Emit a message event to all registered listeners
	fn emit_message(&self, msg_type: &str, message: String) {
		let listeners = self.message_listeners.lock().unwrap();
		for listener in listeners.iter() {
			let _ = listener.call(
				(msg_type.to_string(), message.clone()),
				ThreadsafeFunctionCallMode::NonBlocking,
			);
		}
	}

	/// Emit a step event
	pub fn emit_step(&self, message: String) {
		self.emit_message("step", message);
	}

	/// Emit a warning event
	pub fn emit_warning(&self, message: String) {
		self.emit_message("warning", message);
	}

	/// Emit an error event
	pub fn emit_error(&self, message: String) {
		self.emit_message("error", message);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_container::ProgressId;

	#[test]
	fn test_progress_data_conversion_with_finite_values() {
		let start = Instant::now();
		let state = ProgressState {
			id: ProgressId(1),
			message: "Test progress".to_string(),
			position: 50,
			total: 100,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};

		// Sleep a tiny bit to ensure elapsed time > 0
		std::thread::sleep(std::time::Duration::from_millis(10));

		let progress_data = ProgressData::from(&state);

		assert_eq!(progress_data.position, 50.0);
		assert_eq!(progress_data.total, 100.0);
		assert_eq!(progress_data.percentage, 50.0);
		assert!(progress_data.speed > 0.0);
		assert!(progress_data.estimated_seconds_remaining.is_finite());
		assert!(progress_data.eta.is_finite());
		assert_eq!(progress_data.message, Some("Test progress".to_string()));
	}

	#[test]
	fn test_progress_data_conversion_with_zero_position() {
		let start = Instant::now();
		let state = ProgressState {
			id: ProgressId(1),
			message: "Starting".to_string(),
			position: 0,
			total: 100,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};

		// Sleep to ensure elapsed time > 0
		std::thread::sleep(std::time::Duration::from_millis(10));

		let progress_data = ProgressData::from(&state);

		assert_eq!(progress_data.position, 0.0);
		assert_eq!(progress_data.total, 100.0);
		assert_eq!(progress_data.percentage, 0.0);
		assert_eq!(progress_data.speed, 0.0);
		assert!(progress_data.estimated_seconds_remaining.is_infinite());
		assert!(progress_data.eta.is_infinite());
	}

	#[test]
	fn test_progress_data_conversion_with_completed() {
		let start = Instant::now();
		// Sleep to ensure elapsed time > 0
		std::thread::sleep(std::time::Duration::from_millis(10));

		let state = ProgressState {
			id: ProgressId(1),
			message: "Completed".to_string(),
			position: 100,
			total: 100,
			start,
			next_draw: start,
			next_emit: start,
			finished: true,
		};

		let progress_data = ProgressData::from(&state);

		assert_eq!(progress_data.position, 100.0);
		assert_eq!(progress_data.total, 100.0);
		assert_eq!(progress_data.percentage, 100.0);
		assert!(progress_data.speed > 0.0);
		assert_eq!(progress_data.estimated_seconds_remaining, 0.0);
	}

	#[test]
	fn test_progress_data_percentage_calculation() {
		let start = Instant::now();
		let state = ProgressState {
			id: ProgressId(1),
			message: "Quarter done".to_string(),
			position: 25,
			total: 100,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};

		let progress_data = ProgressData::from(&state);
		assert_eq!(progress_data.percentage, 25.0);
	}

	#[test]
	fn test_progress_new() {
		let progress = Progress::new();
		let listeners = progress.progress_listeners.lock().unwrap();
		assert_eq!(listeners.len(), 0);
	}

	#[test]
	fn test_progress_default() {
		let progress = Progress::default();
		let listeners = progress.progress_listeners.lock().unwrap();
		assert_eq!(listeners.len(), 0);
	}

	#[test]
	fn test_progress_clone() {
		let progress1 = Progress::new();
		let progress2 = progress1.clone();

		// Both should share the same Arc references
		assert!(Arc::ptr_eq(
			&progress1.progress_listeners,
			&progress2.progress_listeners
		));
		assert!(Arc::ptr_eq(&progress1.message_listeners, &progress2.message_listeners));
	}
}
