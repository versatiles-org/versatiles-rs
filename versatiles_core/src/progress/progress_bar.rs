//! Lightweight terminal progress bar without external dependencies.
//!
//! Features:
//! - message
//! - sub-character precision bar (7 partial block steps)
//! - pos/len
//! - percentage
//! - speed (items/sec)
//! - ETA

use super::inner::Inner;
use std::cmp::min;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Type alias for the progress callback function
type ProgressCallback = Option<Box<dyn Fn(ProgressData) + Send + Sync>>;

/// Progress data that can be extracted from a progress bar
#[derive(Debug, Clone)]
pub struct ProgressData {
	pub position: u64,
	pub total: u64,
	pub percentage: f64,
	pub speed: f64,
	pub eta: f64,
	pub message: String,
}

/// A terminal progress bar handle, cloneable and thread-safe.
pub struct ProgressBar {
	inner: Arc<Mutex<Inner>>,
	callback: Arc<Mutex<ProgressCallback>>,
}

impl Default for ProgressBar {
	fn default() -> Self {
		ProgressBar {
			inner: Arc::new(Mutex::new(Inner::default())),
			callback: Arc::new(Mutex::new(None)),
		}
	}
}

impl Clone for ProgressBar {
	fn clone(&self) -> Self {
		ProgressBar {
			inner: self.inner.clone(),
			callback: self.callback.clone(),
		}
	}
}

impl ProgressBar {
	/// Initialize the bar with a message and maximum value.
	pub fn new(message: &str, max_value: u64) -> ProgressBar {
		let progress = ProgressBar {
			inner: Arc::new(Mutex::new(Inner {
				message: message.to_string(),
				len: max_value,
				pos: 0,
				start: Instant::now(),
				finished: false,
				last_draw: Instant::now(),
			})),
			callback: Arc::new(Mutex::new(None)),
		};
		progress.inner.try_lock().unwrap().redraw();
		progress
	}

	/// Extract current progress data without side effects
	pub fn get_data(&self) -> ProgressData {
		let inner = self.inner.lock().unwrap();
		let len = inner.len.max(1);
		let pos = inner.pos.min(len);
		let elapsed = inner.start.elapsed();

		let speed = if elapsed.as_secs_f64() > 0.0 {
			pos as f64 / elapsed.as_secs_f64()
		} else {
			0.0
		};

		let eta = if pos > 0 {
			elapsed.as_secs_f64() * ((len - pos) as f64 / pos as f64).max(0.0)
		} else {
			0.0
		};

		let percentage = (pos as f64 * 100.0 / len as f64).min(100.0);

		ProgressData {
			position: pos,
			total: len,
			percentage,
			speed,
			eta,
			message: inner.message.clone(),
		}
	}

	/// Set a callback to be invoked on progress updates
	pub fn set_callback<F>(&self, callback: F)
	where
		F: Fn(ProgressData) + Send + Sync + 'static,
	{
		let mut cb = self.callback.lock().unwrap();
		*cb = Some(Box::new(callback));
	}

	/// Call the progress callback if one is set
	fn call_callback(&self) {
		let cb = self.callback.lock().unwrap();
		if let Some(ref callback) = *cb {
			let data = self.get_data();
			callback(data);
		}
	}

	/// Set the absolute position.
	pub fn set_position(&self, value: u64) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.pos = min(value, inner.len);
		inner.redraw();
		drop(inner);
		self.call_callback();
	}

	/// Update the maximum length.
	pub fn set_max_value(&self, value: u64) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.len = value;
		if inner.pos > inner.len {
			inner.pos = inner.len;
		}
		inner.redraw();
		drop(inner);
		self.call_callback();
	}

	/// Increment by `value`.
	pub fn inc(&self, value: u64) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.pos = inner.pos.saturating_add(value).min(inner.len);
		inner.redraw();
		drop(inner);
		self.call_callback();
	}

	/// Finish the bar, set position to len and print a final newline.
	pub fn finish(&self) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.pos = inner.len;
		inner.finished = true;
		inner.redraw();

		inner.write("\n");
		drop(inner);
		self.call_callback();
	}

	/// Remove the bar line from the terminal.
	pub fn remove(&self) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.pos = inner.len; // Semantics similar to previous tests
		inner.finished = true;
		inner.write("\r\x1b[2K");
		drop(inner);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_bar_new() {
		let progress = ProgressBar::default();
		let inner = progress.inner.lock().unwrap();
		assert_eq!(inner.len, 0);
		assert_eq!(inner.pos, 0);
	}

	#[test]
	fn test_bar_init() {
		let progress = ProgressBar::new("Test", 100);
		let inner = progress.inner.lock().unwrap();
		assert_eq!(inner.len, 100);
		assert_eq!(inner.message, "Test");
	}

	#[test]
	fn test_bar_set_position() {
		let progress = ProgressBar::new("Test", 100);
		progress.set_position(50);
		let inner = progress.inner.lock().unwrap();
		assert_eq!(inner.pos, 50);
	}

	#[test]
	fn test_bar_inc() {
		let progress = ProgressBar::new("Test", 100);
		progress.set_position(10);
		progress.inc(20);
		let inner = progress.inner.lock().unwrap();
		assert_eq!(inner.pos, 30);
	}

	#[test]
	fn test_bar_finish() {
		let progress = ProgressBar::new("Test", 100);
		progress.set_position(50);
		progress.finish();
		let inner = progress.inner.lock().unwrap();
		assert_eq!(inner.pos, 100);
	}

	#[test]
	fn test_bar_remove() {
		let progress = ProgressBar::new("Test", 100);
		progress.remove();
		let inner = progress.inner.lock().unwrap();
		assert_eq!(inner.pos, 100);
	}
}
