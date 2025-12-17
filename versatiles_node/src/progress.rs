use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use std::sync::{Arc, Mutex};

/// Progress data sent to JavaScript callbacks
#[napi(object)]
#[derive(Clone)]
pub struct ProgressData {
	pub position: f64,
	pub total: f64,
	pub percentage: f64,
	pub speed: f64,
	pub eta: f64,
	pub message: Option<String>,
}

impl From<versatiles_core::progress::ProgressData> for ProgressData {
	fn from(data: versatiles_core::progress::ProgressData) -> Self {
		ProgressData {
			position: data.position as f64,
			total: data.total as f64,
			percentage: data.percentage,
			speed: data.speed,
			eta: data.eta,
			message: Some(data.message),
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
	/// The callback receives ProgressData with position, total, percentage, speed, eta, and message
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
