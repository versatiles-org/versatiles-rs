use napi::{
	bindgen_prelude::*,
	threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;
use std::{
	ops::Div,
	sync::{Arc, Mutex},
	time::{Duration, SystemTime, UNIX_EPOCH},
};
use versatiles_container::ProgressState;

/// Progress information for long-running operations
///
/// Provides real-time progress updates during tile conversion and other
/// operations. All numeric values are floating-point for precision.
#[napi(object)]
#[derive(Clone)]
pub struct ProgressData {
	/// Current position in the operation
	///
	/// Number of items (tiles, bytes, etc.) that have been processed so far.
	/// Compare with `total` to determine progress.
	pub position: f64,

	/// Total number of items to process
	///
	/// The expected total number of items for the complete operation.
	/// May be an estimate and could change during processing.
	pub total: f64,

	/// Completion percentage (0-100)
	///
	/// Calculated as `(position / total) * 100`.
	/// Useful for displaying progress bars.
	///
	/// **Example:** `50.5` means 50.5% complete
	pub percentage: f64,

	/// Processing speed in items per second
	///
	/// Average speed calculated from operation start.
	/// Units match the operation (tiles/sec, bytes/sec, etc.).
	///
	/// **Example:** `1523.4` means 1523.4 items per second
	pub speed: f64,

	/// Estimated seconds until completion
	///
	/// Time remaining based on current speed and remaining items.
	/// Returns `null` if insufficient data to estimate (early in operation).
	///
	/// **Example:** `45.2` means approximately 45 seconds remaining
	pub estimated_seconds_remaining: Option<f64>,

	/// Estimated completion time as JavaScript Date
	///
	/// Timestamp in milliseconds since UNIX epoch (January 1, 1970).
	/// Can be converted to a JavaScript Date object with `new Date(eta)`.
	/// Returns `null` if insufficient data to estimate.
	///
	/// **Example:**
	/// ```javascript
	/// if (progress.eta) {
	///   const completionTime = new Date(progress.eta);
	///   console.log(`Expected completion: ${completionTime.toLocaleTimeString()}`);
	/// }
	/// ```
	pub eta: Option<f64>,

	/// Current operation step or status message
	///
	/// Descriptive text about what the operation is currently doing.
	///
	/// **Examples:**
	/// - `"Reading tiles"`
	/// - `"Compressing data"`
	/// - `"Writing output"`
	pub message: Option<String>,
}

impl From<&ProgressState> for ProgressData {
	fn from(data: &ProgressState) -> Self {
		let speed = data.position as f64 / data.start.elapsed().as_secs_f64();
		let (estimated_seconds_remaining, eta) = if speed > 0.0 && data.position > data.total.div(1000) {
			let remaining_secs = (data.total as f64 - data.position as f64) / speed;

			// Calculate ETA by adding remaining time to current system time
			let eta_ms = SystemTime::now()
				.checked_add(Duration::from_secs_f64(remaining_secs))
				.and_then(|eta_time| eta_time.duration_since(UNIX_EPOCH).ok())
				.map(|d| d.as_secs_f64() * 1000.0);

			(Some(remaining_secs), eta_ms)
		} else {
			(None, None)
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

/// Status or diagnostic message from an operation
///
/// Provides step updates, warnings, and errors during processing.
#[napi(object)]
#[derive(Clone)]
pub struct MessageData {
	/// Message type
	///
	/// One of:
	/// - `"step"`: Normal progress step or status update
	/// - `"warning"`: Non-fatal issue that doesn't stop the operation
	/// - `"error"`: Fatal error that causes operation failure
	#[napi(js_name = "type")]
	pub msg_type: String,

	/// The message text
	///
	/// Human-readable description of the step, warning, or error.
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
	use std::time::Instant;
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
		assert!(progress_data.estimated_seconds_remaining.is_some());
		assert!(progress_data.eta.is_some());
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
		assert_eq!(progress_data.estimated_seconds_remaining, None);
		assert_eq!(progress_data.eta, None);
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
		assert_eq!(progress_data.estimated_seconds_remaining, Some(0.0));
		assert!(progress_data.eta.is_some());
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

	#[test]
	fn test_progress_data_conversion_below_threshold() {
		let start = Instant::now();
		// Sleep to ensure elapsed time > 0
		std::thread::sleep(std::time::Duration::from_millis(10));

		// Position is less than total/1000 (threshold check)
		let state = ProgressState {
			id: ProgressId(1),
			message: "Just started".to_string(),
			position: 1, // 1 < 100000/1000 = 100
			total: 100000,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};

		let progress_data = ProgressData::from(&state);

		// Should not have ETA when below threshold
		assert_eq!(progress_data.position, 1.0);
		assert_eq!(progress_data.total, 100000.0);
		assert!(progress_data.speed > 0.0);
		assert_eq!(progress_data.estimated_seconds_remaining, None);
		assert_eq!(progress_data.eta, None);
	}

	#[test]
	fn test_progress_data_conversion_large_numbers() {
		let start = Instant::now();
		// Sleep to ensure elapsed time > 0
		std::thread::sleep(std::time::Duration::from_millis(50));

		let state = ProgressState {
			id: ProgressId(1),
			message: "Processing many tiles".to_string(),
			position: 5_000_000,
			total: 10_000_000,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};

		let progress_data = ProgressData::from(&state);

		assert_eq!(progress_data.position, 5_000_000.0);
		assert_eq!(progress_data.total, 10_000_000.0);
		assert_eq!(progress_data.percentage, 50.0);
		assert!(progress_data.speed > 0.0);
		assert!(progress_data.estimated_seconds_remaining.is_some());
		assert!(progress_data.eta.is_some());

		// ETA should be in the future
		if let Some(eta_ms) = progress_data.eta {
			let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0;
			assert!(eta_ms > now_ms);
		}
	}

	#[test]
	fn test_progress_data_eta_calculation() {
		let start = Instant::now();
		// Sleep to ensure measurable elapsed time
		std::thread::sleep(std::time::Duration::from_millis(100));

		let state = ProgressState {
			id: ProgressId(1),
			message: "Testing ETA".to_string(),
			position: 1000,
			total: 2000,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};

		let progress_data = ProgressData::from(&state);

		// Verify ETA is calculated and reasonable
		assert!(progress_data.eta.is_some());
		if let Some(eta_ms) = progress_data.eta {
			// ETA should be a valid timestamp (not negative, not too far in future)
			assert!(eta_ms > 0.0);

			// ETA should be in the future but not too far (within 1 hour for this test)
			let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0;
			assert!(eta_ms > now_ms);
			assert!(eta_ms < now_ms + 3_600_000.0); // Within 1 hour
		}
	}

	#[test]
	fn test_progress_data_speed_calculation() {
		let start = Instant::now();
		// Sleep to get measurable elapsed time
		std::thread::sleep(std::time::Duration::from_millis(100));

		let state = ProgressState {
			id: ProgressId(1),
			message: "Speed test".to_string(),
			position: 1000,
			total: 5000,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};

		let progress_data = ProgressData::from(&state);

		// Speed should be position / elapsed_time
		// Note: Using a larger tolerance since timing can vary
		let elapsed_secs = start.elapsed().as_secs_f64();
		let expected_speed = 1000.0 / elapsed_secs;

		// Speed should be within 10% of expected due to timing variations
		let tolerance = expected_speed * 0.1;
		assert!(
			(progress_data.speed - expected_speed).abs() < tolerance,
			"Speed {} not close to expected {} (tolerance: {})",
			progress_data.speed,
			expected_speed,
			tolerance
		);
	}

	#[test]
	fn test_progress_data_message_handling() {
		let start = Instant::now();
		let messages = vec![
			"Step 1: Loading tiles",
			"",
			"Processing batch 5/10",
			"Special characters: ñ, é, 中文, 🚀",
		];

		for msg in messages {
			let state = ProgressState {
				id: ProgressId(1),
				message: msg.to_string(),
				position: 50,
				total: 100,
				start,
				next_draw: start,
				next_emit: start,
				finished: false,
			};

			let progress_data = ProgressData::from(&state);
			assert_eq!(progress_data.message, Some(msg.to_string()));
		}
	}

	#[test]
	fn test_progress_data_percentage_edge_cases() {
		let start = Instant::now();

		// Test 0% case
		let state = ProgressState {
			id: ProgressId(1),
			message: "Starting".to_string(),
			position: 0,
			total: 1000,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};
		let progress_data = ProgressData::from(&state);
		assert_eq!(progress_data.percentage, 0.0);

		// Test 100% case
		let state = ProgressState {
			id: ProgressId(1),
			message: "Done".to_string(),
			position: 1000,
			total: 1000,
			start,
			next_draw: start,
			next_emit: start,
			finished: true,
		};
		let progress_data = ProgressData::from(&state);
		assert_eq!(progress_data.percentage, 100.0);

		// Test fractional percentage
		let state = ProgressState {
			id: ProgressId(1),
			message: "Third".to_string(),
			position: 33,
			total: 100,
			start,
			next_draw: start,
			next_emit: start,
			finished: false,
		};
		let progress_data = ProgressData::from(&state);
		assert_eq!(progress_data.percentage, 33.0);
	}

	#[test]
	fn test_message_data_creation() {
		let msg = MessageData {
			msg_type: "warning".to_string(),
			message: "Test warning message".to_string(),
		};

		assert_eq!(msg.msg_type, "warning");
		assert_eq!(msg.message, "Test warning message");
	}

	#[test]
	fn test_message_data_clone() {
		let msg1 = MessageData {
			msg_type: "error".to_string(),
			message: "Test error".to_string(),
		};

		let msg2 = msg1.clone();

		assert_eq!(msg1.msg_type, msg2.msg_type);
		assert_eq!(msg1.message, msg2.message);
	}

	#[test]
	fn test_progress_data_clone() {
		let progress_data1 = ProgressData {
			position: 50.0,
			total: 100.0,
			percentage: 50.0,
			speed: 10.5,
			estimated_seconds_remaining: Some(5.0),
			eta: Some(1234567890.0),
			message: Some("Test".to_string()),
		};

		let progress_data2 = progress_data1.clone();

		assert_eq!(progress_data1.position, progress_data2.position);
		assert_eq!(progress_data1.total, progress_data2.total);
		assert_eq!(progress_data1.percentage, progress_data2.percentage);
		assert_eq!(progress_data1.speed, progress_data2.speed);
		assert_eq!(
			progress_data1.estimated_seconds_remaining,
			progress_data2.estimated_seconds_remaining
		);
		assert_eq!(progress_data1.eta, progress_data2.eta);
		assert_eq!(progress_data1.message, progress_data2.message);
	}

	#[test]
	fn test_emit_progress_no_listeners() {
		// Verify emit_progress doesn't panic with no listeners
		let progress = Progress::new();
		let data = ProgressData {
			position: 50.0,
			total: 100.0,
			percentage: 50.0,
			speed: 10.5,
			estimated_seconds_remaining: Some(5.0),
			eta: Some(1234567890.0),
			message: Some("Test".to_string()),
		};

		// Should not panic
		progress.emit_progress(data);
	}

	#[test]
	fn test_emit_step_no_listeners() {
		// Verify emit_step doesn't panic with no listeners
		let progress = Progress::new();

		// Should not panic
		progress.emit_step("Step 1: Processing tiles".to_string());
	}

	#[test]
	fn test_emit_warning_no_listeners() {
		// Verify emit_warning doesn't panic with no listeners
		let progress = Progress::new();

		// Should not panic
		progress.emit_warning("Warning: Low memory".to_string());
	}

	#[test]
	fn test_emit_error_no_listeners() {
		// Verify emit_error doesn't panic with no listeners
		let progress = Progress::new();

		// Should not panic
		progress.emit_error("Error: File not found".to_string());
	}

	#[test]
	fn test_emit_multiple_progress_events() {
		// Verify multiple emit_progress calls work correctly
		let progress = Progress::new();

		for i in 0..10 {
			let data = ProgressData {
				position: (i * 10) as f64,
				total: 100.0,
				percentage: (i * 10) as f64,
				speed: 10.5,
				estimated_seconds_remaining: Some(5.0),
				eta: Some(1234567890.0),
				message: Some(format!("Processing item {}", i)),
			};

			// Should not panic
			progress.emit_progress(data);
		}
	}

	#[test]
	fn test_emit_multiple_message_types() {
		// Verify multiple different message types can be emitted
		let progress = Progress::new();

		// Emit different message types in sequence
		progress.emit_step("Step 1: Loading".to_string());
		progress.emit_step("Step 2: Processing".to_string());
		progress.emit_warning("Warning: Slow processing".to_string());
		progress.emit_step("Step 3: Writing".to_string());
		progress.emit_error("Error: Write failed".to_string());

		// All should complete without panicking
	}

	#[test]
	fn test_emit_with_empty_messages() {
		// Verify empty strings are handled correctly
		let progress = Progress::new();

		progress.emit_step("".to_string());
		progress.emit_warning("".to_string());
		progress.emit_error("".to_string());

		// Should not panic
	}

	#[test]
	fn test_emit_with_special_characters() {
		// Verify special characters in messages are handled correctly
		let progress = Progress::new();

		progress.emit_step("Step: Processing 中文 tiles".to_string());
		progress.emit_warning("Warning: File 'test.txt' not found".to_string());
		progress.emit_error("Error: Invalid char 🚀".to_string());

		// Should not panic
	}

	#[test]
	fn test_emit_with_long_messages() {
		// Verify long messages are handled correctly
		let progress = Progress::new();

		let long_message = "A".repeat(10000);
		progress.emit_step(long_message.clone());
		progress.emit_warning(long_message.clone());
		progress.emit_error(long_message);

		// Should not panic
	}

	#[test]
	fn test_emit_from_cloned_progress() {
		// Verify emitting from cloned Progress instances works
		let progress1 = Progress::new();
		let progress2 = progress1.clone();

		// Emit from both instances
		progress1.emit_step("From progress1".to_string());
		progress2.emit_warning("From progress2".to_string());

		// Both should work without panicking since they share the same Arc
	}

	#[test]
	fn test_emit_progress_with_various_percentages() {
		// Test emitting progress at various completion levels
		let progress = Progress::new();

		let test_cases = vec![
			(0.0, 0.0),
			(25.0, 25.0),
			(50.0, 50.0),
			(75.0, 75.0),
			(99.9, 99.9),
			(100.0, 100.0),
		];

		for (position, percentage) in test_cases {
			let data = ProgressData {
				position,
				total: 100.0,
				percentage,
				speed: 10.0,
				estimated_seconds_remaining: None,
				eta: None,
				message: Some(format!("{}% complete", percentage)),
			};

			progress.emit_progress(data);
		}
	}

	#[test]
	fn test_emit_progress_with_none_values() {
		// Test emitting progress with None optional fields
		let progress = Progress::new();

		let data = ProgressData {
			position: 50.0,
			total: 100.0,
			percentage: 50.0,
			speed: 0.0,
			estimated_seconds_remaining: None,
			eta: None,
			message: None,
		};

		// Should not panic with None values
		progress.emit_progress(data);
	}

	#[test]
	fn test_emit_message_types_distinction() {
		// Verify different message types are emitted correctly
		let progress = Progress::new();

		// Each type should be callable independently
		progress.emit_step("This is a step".to_string());
		progress.emit_warning("This is a warning".to_string());
		progress.emit_error("This is an error".to_string());

		// Verify we can mix them
		for i in 0..5 {
			match i % 3 {
				0 => progress.emit_step(format!("Step {}", i)),
				1 => progress.emit_warning(format!("Warning {}", i)),
				_ => progress.emit_error(format!("Error {}", i)),
			}
		}
	}

	#[test]
	fn test_concurrent_emits() {
		use std::sync::Arc;
		use std::thread;

		// Test that Progress is thread-safe for emits
		let progress = Arc::new(Progress::new());
		let mut handles = vec![];

		// Spawn multiple threads emitting different events
		for i in 0..10 {
			let progress_clone = Arc::clone(&progress);
			let handle = thread::spawn(move || {
				for j in 0..10 {
					match (i + j) % 3 {
						0 => progress_clone.emit_step(format!("Thread {} step {}", i, j)),
						1 => progress_clone.emit_warning(format!("Thread {} warning {}", i, j)),
						_ => progress_clone.emit_error(format!("Thread {} error {}", i, j)),
					}
				}
			});
			handles.push(handle);
		}

		// Wait for all threads to complete
		for handle in handles {
			handle.join().unwrap();
		}

		// If we get here, no panics occurred
	}

	#[test]
	fn test_emit_progress_concurrent() {
		use std::sync::Arc;
		use std::thread;

		// Test that emit_progress is thread-safe
		let progress = Arc::new(Progress::new());
		let mut handles = vec![];

		for i in 0..10 {
			let progress_clone = Arc::clone(&progress);
			let handle = thread::spawn(move || {
				for j in 0..100 {
					let data = ProgressData {
						position: (i * 100 + j) as f64,
						total: 1000.0,
						percentage: ((i * 100 + j) as f64 / 10.0),
						speed: 50.0,
						estimated_seconds_remaining: Some(10.0),
						eta: Some(1234567890.0),
						message: Some(format!("Thread {} item {}", i, j)),
					};
					progress_clone.emit_progress(data);
				}
			});
			handles.push(handle);
		}

		for handle in handles {
			handle.join().unwrap();
		}
	}
}
