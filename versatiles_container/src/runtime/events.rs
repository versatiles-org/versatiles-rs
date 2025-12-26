//! Event system for runtime events
//!
//! Provides a unified event bus for all runtime events including:
//! - Logging events
//! - Progress updates
//! - Step/stage messages
//! - Warnings and errors

use crate::ProgressState;
use arc_swap::ArcSwap;
use std::sync::Arc;

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
/// Uses lock-free arc-swap for optimal performance when emitting frequent events.
#[derive(Clone)]
pub struct EventBus {
	listeners: Arc<ArcSwap<Vec<EventListener>>>,
}

impl EventBus {
	/// Create a new event bus
	pub fn new() -> Self {
		Self {
			listeners: Arc::new(ArcSwap::from_pointee(Vec::new())),
		}
	}

	/// Register an event listener
	///
	/// Returns a listener ID that can be used to unsubscribe (future enhancement).
	/// The listener will be called for all events emitted on this bus.
	/// Uses read-copy-update (RCU) for lock-free hot-reload.
	pub fn subscribe<F>(&self, listener: F) -> ListenerId
	where
		F: Fn(&Event) + Send + Sync + 'static,
	{
		let listener = Arc::new(listener);
		let id = self.listeners.load().len();
		self.listeners.rcu(|old| {
			let mut new = (**old).clone();
			new.push(listener.clone());
			new
		});
		ListenerId(id)
	}

	/// Emit an event to all listeners
	///
	/// Listeners are called synchronously in the order they were registered.
	/// If a listener panics, the panic is caught and other listeners continue.
	/// Lock-free load for optimal performance on frequent emissions.
	pub fn emit(&self, event: Event) {
		let listeners = self.listeners.load();
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ProgressId;
	use std::sync::{Arc, Mutex};

	#[test]
	fn test_event_bus_new() {
		let bus = EventBus::new();
		assert_eq!(bus.listeners.load().len(), 0);
	}

	#[test]
	fn test_event_bus_default() {
		let bus = EventBus::default();
		assert_eq!(bus.listeners.load().len(), 0);
	}

	#[test]
	fn test_event_bus_subscribe() {
		let bus = EventBus::new();
		let counter = Arc::new(Mutex::new(0));
		let counter_clone = counter.clone();

		let _id = bus.subscribe(move |_event| {
			*counter_clone.lock().unwrap() += 1;
		});

		bus.emit(Event::Step {
			message: "Test".to_string(),
		});

		assert_eq!(*counter.lock().unwrap(), 1);
	}

	#[test]
	fn test_event_bus_multiple_subscribers() {
		let bus = EventBus::new();
		let counter1 = Arc::new(Mutex::new(0));
		let counter2 = Arc::new(Mutex::new(0));

		let counter1_clone = counter1.clone();
		let counter2_clone = counter2.clone();

		bus.subscribe(move |_event| {
			*counter1_clone.lock().unwrap() += 1;
		});

		bus.subscribe(move |_event| {
			*counter2_clone.lock().unwrap() += 10;
		});

		bus.emit(Event::Step {
			message: "Test".to_string(),
		});

		assert_eq!(*counter1.lock().unwrap(), 1);
		assert_eq!(*counter2.lock().unwrap(), 10);
	}

	#[test]
	fn test_event_bus_log_event() {
		let bus = EventBus::new();
		let captured = Arc::new(Mutex::new(Vec::new()));
		let captured_clone = captured.clone();

		bus.subscribe(move |event| {
			if let Event::Log { level, target, message } = event {
				captured_clone
					.lock()
					.unwrap()
					.push((*level, target.clone(), message.clone()));
			}
		});

		bus.log(LogLevel::Info, "test_target", "Test message".to_string());

		let events = captured.lock().unwrap();
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].0, LogLevel::Info);
		assert_eq!(events[0].1, "test_target");
		assert_eq!(events[0].2, "Test message");
	}

	#[test]
	fn test_event_bus_progress_event() {
		let bus = EventBus::new();
		let captured = Arc::new(Mutex::new(Vec::new()));
		let captured_clone = captured.clone();

		bus.subscribe(move |event| {
			if let Event::Progress { data } = event {
				captured_clone.lock().unwrap().push(data.position);
			}
		});

		let state = ProgressState {
			id: ProgressId(1),
			message: "Test".to_string(),
			position: 50,
			total: 100,
			start: std::time::Instant::now(),
			next_draw: std::time::Instant::now(),
			next_emit: std::time::Instant::now(),
			finished: false,
		};

		bus.progress(state);

		let positions = captured.lock().unwrap();
		assert_eq!(positions.len(), 1);
		assert_eq!(positions[0], 50);
	}

	#[test]
	fn test_event_bus_step_event() {
		let bus = EventBus::new();
		let captured = Arc::new(Mutex::new(Vec::new()));
		let captured_clone = captured.clone();

		bus.subscribe(move |event| {
			if let Event::Step { message } = event {
				captured_clone.lock().unwrap().push(message.clone());
			}
		});

		bus.step("Step 1".to_string());
		bus.step("Step 2".to_string());

		let messages = captured.lock().unwrap();
		assert_eq!(messages.len(), 2);
		assert_eq!(messages[0], "Step 1");
		assert_eq!(messages[1], "Step 2");
	}

	#[test]
	fn test_event_bus_warning_event() {
		let bus = EventBus::new();
		let captured = Arc::new(Mutex::new(Vec::new()));
		let captured_clone = captured.clone();

		bus.subscribe(move |event| {
			if let Event::Warning { message } = event {
				captured_clone.lock().unwrap().push(message.clone());
			}
		});

		bus.warn("Warning message".to_string());

		let warnings = captured.lock().unwrap();
		assert_eq!(warnings.len(), 1);
		assert_eq!(warnings[0], "Warning message");
	}

	#[test]
	fn test_event_bus_error_event() {
		let bus = EventBus::new();
		let captured = Arc::new(Mutex::new(Vec::new()));
		let captured_clone = captured.clone();

		bus.subscribe(move |event| {
			if let Event::Error { message } = event {
				captured_clone.lock().unwrap().push(message.clone());
			}
		});

		bus.error("Error message".to_string());

		let errors = captured.lock().unwrap();
		assert_eq!(errors.len(), 1);
		assert_eq!(errors[0], "Error message");
	}

	#[test]
	fn test_event_bus_clone() {
		let bus1 = EventBus::new();
		let bus2 = bus1.clone();

		let counter = Arc::new(Mutex::new(0));
		let counter_clone = counter.clone();

		bus1.subscribe(move |_event| {
			*counter_clone.lock().unwrap() += 1;
		});

		// Emitting on bus2 should trigger listeners registered on bus1
		bus2.step("Test".to_string());

		assert_eq!(*counter.lock().unwrap(), 1);
	}

	#[test]
	fn test_event_bus_panic_handling() {
		let bus = EventBus::new();
		let counter = Arc::new(Mutex::new(0));
		let counter_clone = counter.clone();

		// First listener panics
		bus.subscribe(|_event| {
			panic!("Test panic");
		});

		// Second listener should still run
		bus.subscribe(move |_event| {
			*counter_clone.lock().unwrap() += 1;
		});

		bus.step("Test".to_string());

		// Counter should still be incremented despite the panic
		assert_eq!(*counter.lock().unwrap(), 1);
	}

	#[test]
	fn test_log_level_equality() {
		assert_eq!(LogLevel::Error, LogLevel::Error);
		assert_eq!(LogLevel::Warn, LogLevel::Warn);
		assert_eq!(LogLevel::Info, LogLevel::Info);
		assert_eq!(LogLevel::Debug, LogLevel::Debug);
		assert_eq!(LogLevel::Trace, LogLevel::Trace);

		assert_ne!(LogLevel::Error, LogLevel::Warn);
		assert_ne!(LogLevel::Info, LogLevel::Debug);
	}

	#[test]
	fn test_listener_id_equality() {
		let id1 = ListenerId(0);
		let id2 = ListenerId(0);
		let id3 = ListenerId(1);

		assert_eq!(id1, id2);
		assert_ne!(id1, id3);
	}

	#[test]
	fn test_log_adapter_creation() {
		let bus = EventBus::new();
		let _adapter = bus.create_log_adapter();
		// Adapter should be created successfully
	}

	#[test]
	fn test_log_adapter_forwards_events() {
		use log::Log;

		let bus = EventBus::new();
		let captured = Arc::new(Mutex::new(Vec::new()));
		let captured_clone = captured.clone();

		bus.subscribe(move |event| {
			if let Event::Log { level, target, message } = event {
				captured_clone
					.lock()
					.unwrap()
					.push((*level, target.clone(), message.clone()));
			}
		});

		let adapter = LogAdapter::new(bus);

		// Create a log record
		let record = log::Record::builder()
			.level(log::Level::Warn)
			.target("test_target")
			.args(format_args!("Test warning"))
			.build();

		adapter.log(&record);

		let events = captured.lock().unwrap();
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].0, LogLevel::Warn);
		assert_eq!(events[0].1, "test_target");
		assert_eq!(events[0].2, "Test warning");
	}

	#[test]
	fn test_log_adapter_level_mapping() {
		use log::Log;

		let bus = EventBus::new();
		let captured = Arc::new(Mutex::new(Vec::new()));
		let captured_clone = captured.clone();

		bus.subscribe(move |event| {
			if let Event::Log { level, .. } = event {
				captured_clone.lock().unwrap().push(*level);
			}
		});

		let adapter = LogAdapter::new(bus);

		// Test all log levels
		for (log_level, _expected_level) in [
			(log::Level::Error, LogLevel::Error),
			(log::Level::Warn, LogLevel::Warn),
			(log::Level::Info, LogLevel::Info),
			(log::Level::Debug, LogLevel::Debug),
			(log::Level::Trace, LogLevel::Trace),
		] {
			let record = log::Record::builder()
				.level(log_level)
				.target("test")
				.args(format_args!("msg"))
				.build();

			adapter.log(&record);
		}

		let levels = captured.lock().unwrap();
		assert_eq!(levels.len(), 5);
		assert_eq!(levels[0], LogLevel::Error);
		assert_eq!(levels[1], LogLevel::Warn);
		assert_eq!(levels[2], LogLevel::Info);
		assert_eq!(levels[3], LogLevel::Debug);
		assert_eq!(levels[4], LogLevel::Trace);
	}

	#[test]
	fn test_log_adapter_enabled() {
		use log::Log;

		let bus = EventBus::new();
		let adapter = LogAdapter::new(bus);

		let metadata = log::MetadataBuilder::new().level(log::Level::Info).build();
		assert!(adapter.enabled(&metadata));
	}
}
