use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ErrorStrategy, ThreadsafeFunction, ThreadsafeFunctionCallMode};
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

/// Event types that can be emitted by Progress
#[derive(Clone)]
pub enum ProgressEvent {
	Progress(ProgressData),
	Step(String),
	Warning(String),
	Error(String),
	Complete,
}

type EventCallback = ThreadsafeFunction<ProgressEvent, ErrorStrategy::Fatal>;

/// Progress monitor for long-running operations
///
/// This class allows monitoring the progress of tile conversion and other
/// long-running operations through event listeners.
#[napi]
#[derive(Clone)]
pub struct Progress {
	// Event listeners stored by event name
	progress_listeners: Arc<Mutex<Vec<EventCallback>>>,
	step_listeners: Arc<Mutex<Vec<EventCallback>>>,
	warning_listeners: Arc<Mutex<Vec<EventCallback>>>,
	error_listeners: Arc<Mutex<Vec<EventCallback>>>,
	complete_listeners: Arc<Mutex<Vec<EventCallback>>>,

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
			step_listeners: Arc::new(Mutex::new(Vec::new())),
			warning_listeners: Arc::new(Mutex::new(Vec::new())),
			error_listeners: Arc::new(Mutex::new(Vec::new())),
			complete_listeners: Arc::new(Mutex::new(Vec::new())),
			completion_tx: Arc::new(Mutex::new(Some(tx))),
			completion_rx: Arc::new(Mutex::new(Some(rx))),
		}
	}

	/// Register an event listener
	///
	/// Supported events:
	/// - 'progress': Emitted with ProgressData on progress updates
	/// - 'step': Emitted with string message when operation phase changes
	/// - 'warning': Emitted with warning message
	/// - 'error': Emitted with error message
	/// - 'complete': Emitted when operation completes successfully
	#[napi]
	pub fn on(&self, event: String, callback: JsFunction) -> Result<&Self> {
		// Create a threadsafe function based on event type
		let tsfn: EventCallback = callback.create_threadsafe_function(0, |ctx| {
			let event = ctx.value;

			match event {
				ProgressEvent::Progress(data) => {
					let mut obj = ctx.env.create_object()?;
					obj.set("position", data.position)?;
					obj.set("total", data.total)?;
					obj.set("percentage", data.percentage)?;
					obj.set("speed", data.speed)?;
					obj.set("eta", data.eta)?;
					if let Some(msg) = data.message {
						obj.set("message", msg)?;
					}
					Ok(vec![obj.into_unknown()])
				}
				ProgressEvent::Step(msg) | ProgressEvent::Warning(msg) | ProgressEvent::Error(msg) => {
					let js_string = ctx.env.create_string(&msg)?;
					Ok(vec![js_string.into_unknown()])
				}
				ProgressEvent::Complete => Ok(vec![]),
			}
		})?;

		// Store the callback in the appropriate listener list
		match event.as_str() {
			"progress" => {
				let mut listeners = self.progress_listeners.lock().unwrap();
				listeners.push(tsfn);
			}
			"step" => {
				let mut listeners = self.step_listeners.lock().unwrap();
				listeners.push(tsfn);
			}
			"warning" => {
				let mut listeners = self.warning_listeners.lock().unwrap();
				listeners.push(tsfn);
			}
			"error" => {
				let mut listeners = self.error_listeners.lock().unwrap();
				listeners.push(tsfn);
			}
			"complete" => {
				let mut listeners = self.complete_listeners.lock().unwrap();
				listeners.push(tsfn);
			}
			_ => {
				return Err(Error::from_reason(format!("Unknown event type: {}", event)));
			}
		}

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
			let _ = listener.call(
				ProgressEvent::Progress(data.clone()),
				ThreadsafeFunctionCallMode::NonBlocking,
			);
		}
	}

	/// Emit a step event
	pub fn emit_step(&self, message: String) {
		let listeners = self.step_listeners.lock().unwrap();
		for listener in listeners.iter() {
			let _ = listener.call(
				ProgressEvent::Step(message.clone()),
				ThreadsafeFunctionCallMode::NonBlocking,
			);
		}
	}

	/// Emit a warning event
	pub fn emit_warning(&self, message: String) {
		let listeners = self.warning_listeners.lock().unwrap();
		for listener in listeners.iter() {
			let _ = listener.call(
				ProgressEvent::Warning(message.clone()),
				ThreadsafeFunctionCallMode::NonBlocking,
			);
		}
	}

	/// Emit an error event
	pub fn emit_error(&self, message: String) {
		let listeners = self.error_listeners.lock().unwrap();
		for listener in listeners.iter() {
			let _ = listener.call(
				ProgressEvent::Error(message.clone()),
				ThreadsafeFunctionCallMode::NonBlocking,
			);
		}
	}

	/// Emit a complete event
	pub fn emit_complete(&self) {
		let listeners = self.complete_listeners.lock().unwrap();
		for listener in listeners.iter() {
			let _ = listener.call(ProgressEvent::Complete, ThreadsafeFunctionCallMode::NonBlocking);
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
