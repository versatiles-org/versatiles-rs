use crate::progress::{Progress, ProgressData};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use versatiles_core::progress::ProgressBar;

/// Bridge between Rust ProgressBar and JavaScript Progress events
///
/// This struct wraps a ProgressBar and emits events to a JavaScript Progress object.
/// It throttles updates to avoid overwhelming the JavaScript event loop.
pub struct ProgressCallback {
	progress_bar: ProgressBar,
	js_progress: Arc<Progress>,
	last_emit: Arc<Mutex<Instant>>,
}

impl ProgressCallback {
	/// Create a new ProgressCallback
	///
	/// # Arguments
	/// * `message` - Description of the operation
	/// * `max_value` - Total number of items to process
	/// * `js_progress` - JavaScript Progress object to emit events to
	pub fn new(message: &str, max_value: u64, js_progress: Arc<Progress>) -> Self {
		let progress_bar = ProgressBar::new(message, max_value);

		// Set up the callback on the progress bar
		let js_progress_clone = js_progress.clone();
		let last_emit = Arc::new(Mutex::new(Instant::now()));
		let last_emit_clone = last_emit.clone();

		progress_bar.set_callback(move |data| {
			// Throttle updates to ~100ms intervals
			let mut last = last_emit_clone.lock().unwrap();
			if last.elapsed().as_millis() < 100 {
				return;
			}
			*last = Instant::now();
			drop(last);

			// Convert and emit the progress data
			let js_data: ProgressData = data.into();
			js_progress_clone.emit_progress(js_data);
		});

		ProgressCallback {
			progress_bar,
			js_progress,
			last_emit,
		}
	}

	/// Get a reference to the underlying ProgressBar
	pub fn progress_bar(&self) -> &ProgressBar {
		&self.progress_bar
	}

	/// Increment the progress
	#[allow(dead_code)]
	pub fn inc(&self, value: u64) {
		self.progress_bar.inc(value);
	}

	/// Set the absolute position
	#[allow(dead_code)]
	pub fn set_position(&self, value: u64) {
		self.progress_bar.set_position(value);
	}

	/// Set the maximum value
	#[allow(dead_code)]
	pub fn set_max_value(&self, value: u64) {
		self.progress_bar.set_max_value(value);
	}

	/// Emit a step change event
	#[allow(dead_code)]
	pub fn emit_step(&self, message: &str) {
		self.js_progress.emit_step(message.to_string());
	}

	/// Emit a warning
	#[allow(dead_code)]
	pub fn emit_warning(&self, message: &str) {
		self.js_progress.emit_warning(message.to_string());
	}

	/// Finish the progress and mark as complete
	#[allow(dead_code)]
	pub fn finish(&self) {
		self.progress_bar.finish();

		// Emit final progress update
		let data: ProgressData = self.progress_bar.get_data().into();
		self.js_progress.emit_progress(data);

		self.js_progress.complete();
	}

	/// Fail the progress with an error
	#[allow(dead_code)]
	pub fn fail(&self, error: anyhow::Error) {
		self.js_progress.fail(error);
	}
}

impl Clone for ProgressCallback {
	fn clone(&self) -> Self {
		ProgressCallback {
			progress_bar: self.progress_bar.clone(),
			js_progress: self.js_progress.clone(),
			last_emit: self.last_emit.clone(),
		}
	}
}
