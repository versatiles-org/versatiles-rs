//! Event system for runtime events
//!
//! Provides a unified event bus for all runtime events including:
//! - Logging events
//! - Progress updates
//! - Step/stage messages
//! - Warnings and errors

use crate::ProgressState;
use std::sync::{Arc, RwLock};

/// Event types that can be emitted by the runtime
#[derive(Debug, Clone)]
pub enum Event {
	/// Logging event with level and message
	Log {
		level: LogLevel,
		target: String,
		message: String,
	},

	/// Progress update event
	Progress { data: ProgressState },

	/// Step/stage message
	Step { message: String },

	/// Warning message
	Warning { message: String },

	/// Error message
	Error { message: String },
}

/// Log level for logging events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
	Error,
	Warn,
	Info,
	Debug,
	Trace,
}

/// Unique identifier for event listeners
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(usize);

type EventListener = Arc<dyn Fn(&Event) + Send + Sync>;

/// Thread-safe event bus for runtime events
///
/// The event bus allows subscribers to register listeners that will be called
/// when events are emitted. All listener calls are synchronous and blocking.
#[derive(Clone)]
pub struct EventBus {
	listeners: Arc<RwLock<Vec<EventListener>>>,
}

impl EventBus {
	/// Create a new event bus
	pub fn new() -> Self {
		Self {
			listeners: Arc::new(RwLock::new(Vec::new())),
		}
	}

	/// Register an event listener
	///
	/// Returns a listener ID that can be used to unsubscribe (future enhancement).
	/// The listener will be called for all events emitted on this bus.
	pub fn subscribe<F>(&self, listener: F) -> ListenerId
	where
		F: Fn(&Event) + Send + Sync + 'static,
	{
		let mut listeners = self.listeners.write().unwrap();
		let id = listeners.len();
		listeners.push(Arc::new(listener));
		ListenerId(id)
	}

	/// Emit an event to all listeners
	///
	/// Listeners are called synchronously in the order they were registered.
	/// If a listener panics, the panic is caught and other listeners continue.
	pub fn emit(&self, event: Event) {
		let listeners = self.listeners.read().unwrap();
		for listener in listeners.iter() {
			// Catch panics to prevent one bad listener from breaking others
			let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
				listener(&event);
			}));
		}
	}

	/// Emit a log event
	pub fn log(&self, level: LogLevel, target: &str, message: String) {
		self.emit(Event::Log {
			level,
			target: target.to_string(),
			message,
		});
	}

	/// Emit a progress event
	pub fn progress(&self, data: ProgressState) {
		self.emit(Event::Progress { data });
	}

	/// Emit a step event
	pub fn step(&self, message: String) {
		self.emit(Event::Step { message });
	}

	/// Emit a warning event
	pub fn warn(&self, message: String) {
		self.emit(Event::Warning { message });
	}

	/// Emit an error event
	pub fn error(&self, message: String) {
		self.emit(Event::Error { message });
	}
}

impl Default for EventBus {
	fn default() -> Self {
		Self::new()
	}
}

/// Adapter to forward log crate events to the event bus
pub struct LogAdapter {
	event_bus: EventBus,
}

impl LogAdapter {
	/// Create a new log adapter
	pub fn new(event_bus: EventBus) -> Self {
		Self { event_bus }
	}
}

impl log::Log for LogAdapter {
	fn enabled(&self, _metadata: &log::Metadata) -> bool {
		true
	}

	fn log(&self, record: &log::Record) {
		let level = match record.level() {
			log::Level::Error => LogLevel::Error,
			log::Level::Warn => LogLevel::Warn,
			log::Level::Info => LogLevel::Info,
			log::Level::Debug => LogLevel::Debug,
			log::Level::Trace => LogLevel::Trace,
		};

		self.event_bus.log(level, record.target(), format!("{}", record.args()));
	}

	fn flush(&self) {}
}

impl EventBus {
	/// Create a log adapter that forwards log crate events to the event bus
	pub fn create_log_adapter(&self) -> LogAdapter {
		LogAdapter::new(self.clone())
	}
}
