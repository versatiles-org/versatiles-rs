use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

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

// Type aliases for the three different callback types
// Note: Using the full signature because build_callback sets CalleeHandled=false
type ProgressCallback = ThreadsafeFunction<ProgressData, Unknown<'static>, ProgressData, Status, false>;
type MessageCallback = ThreadsafeFunction<(String, String), Unknown<'static>, (String, String), Status, false>;
type CompleteCallback = ThreadsafeFunction<(), Unknown<'static>, (), Status, false>;

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
	complete_listeners: Arc<Mutex<Vec<CompleteCallback>>>,

	// Channel to signal completion
	completion_tx: Arc<Mutex<Option<oneshot::Sender<Result<()>>>>>,
	completion_rx: Arc<Mutex<Option<oneshot::Receiver<Result<()>>>>>,
}

#[napi]
impl Progress {
	/// Create a new Progress instance
	pub fn new() -> Self {
		let (tx, rx) = oneshot::channel();

		Progress {
			progress_listeners: Arc::new(Mutex::new(Vec::new())),
			message_listeners: Arc::new(Mutex::new(Vec::new())),
			complete_listeners: Arc::new(Mutex::new(Vec::new())),
			completion_tx: Arc::new(Mutex::new(Some(tx))),
			completion_rx: Arc::new(Mutex::new(Some(rx))),
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
			.build_callback(|ctx| Ok(ctx.value))?;
		let mut listeners = self.message_listeners.lock().unwrap();
		listeners.push(tsfn);
		Ok(self)
	}

	/// Register a complete event listener
	///
	/// The callback is called with no arguments when the operation completes
	#[napi(ts_args_type = "callback: () => void")]
	pub fn on_complete(&self, callback: Function<'static>) -> Result<&Self> {
		let tsfn = callback
			.build_threadsafe_function::<()>()
			.build_callback(|_ctx| Ok(()))?;
		let mut listeners = self.complete_listeners.lock().unwrap();
		listeners.push(tsfn);
		Ok(self)
	}

	/// Returns a Promise that resolves when the operation completes
	#[napi]
	pub async fn done(&self) -> Result<()> {
		let rx = {
			let mut completion_rx = self.completion_rx.lock().unwrap();
			completion_rx.take()
		};

		match rx {
			Some(receiver) => match receiver.await {
				Ok(Ok(())) => Ok(()),
				Ok(Err(e)) => Err(Error::from_reason(e.to_string())),
				Err(_) => Err(Error::from_reason("Operation was cancelled")),
			},
			None => Err(Error::from_reason("done() can only be called once")),
		}
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

	/// Emit a complete event
	pub fn emit_complete(&self) {
		let listeners = self.complete_listeners.lock().unwrap();
		for listener in listeners.iter() {
			let _ = listener.call((), ThreadsafeFunctionCallMode::NonBlocking);
		}
	}

	/// Signal that the operation has completed successfully
	pub fn complete(&self) {
		self.emit_complete();

		let mut tx = self.completion_tx.lock().unwrap();
		if let Some(sender) = tx.take() {
			let _ = sender.send(Ok(()));
		}
	}

	/// Signal that the operation has failed with an error
	pub fn fail(&self, error: anyhow::Error) {
		self.emit_error(error.to_string());

		let mut tx = self.completion_tx.lock().unwrap();
		if let Some(sender) = tx.take() {
			let _ = sender.send(Err(Error::from_reason(error.to_string())));
		}
	}
}
