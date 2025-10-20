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

/// A terminal progress bar handle, cloneable and thread-safe.
pub struct ProgressBar {
	inner: Arc<Mutex<Inner>>,
}

impl Default for ProgressBar {
	fn default() -> Self {
		ProgressBar {
			inner: Arc::new(Mutex::new(Inner::default())),
		}
	}
}

impl Clone for ProgressBar {
	fn clone(&self) -> Self {
		ProgressBar {
			inner: self.inner.clone(),
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
		};
		progress.inner.try_lock().unwrap().redraw();
		progress
	}

	/// Set the absolute position.
	pub fn set_position(&self, value: u64) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.pos = min(value, inner.len);
		inner.redraw();
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
	}

	/// Increment by `value`.
	pub fn inc(&self, value: u64) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.pos = inner.pos.saturating_add(value).min(inner.len);
		inner.redraw();
	}

	/// Finish the bar, set position to len and print a final newline.
	pub fn finish(&self) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.pos = inner.len;
		inner.finished = true;
		inner.redraw();

		inner.write("\n");
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
