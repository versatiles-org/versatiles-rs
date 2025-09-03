//! This module provides a `ProgressBar` struct that implements a terminal-based progress bar.
//!
//! # Overview
//!
//! The `ProgressBar` struct represents a progress bar that can be used to display the progress of a task in the terminal.
//! It implements the `ProgressTrait` trait, which provides methods for initializing the progress bar, setting its position,
//! incrementing its value, and finishing or removing it.
//!
//! # Examples
//!
//! ```rust
//! use versatiles_core::progress::get_progress_bar;
//!
//! let progress = get_progress_bar("Processing", 100);
//! progress.set_position(50);
//! progress.inc(10);
//! progress.finish();
//! ```

use indicatif::{ProgressBar as IndicatifProgressBar, ProgressStyle};
use std::{sync::Arc, time::Duration};

/// A terminal progress bar handle, cloneable and thread-safe via indicatif.
#[derive(Clone)]
pub struct ProgressBar {
	bar: Arc<IndicatifProgressBar>,
}

impl ProgressBar {
	pub fn new() -> Self {
		#[cfg(all(not(feature = "test"), feature = "cli"))]
		let bar = IndicatifProgressBar::new(0);
		#[cfg(any(feature = "test", not(feature = "cli")))]
		let bar = IndicatifProgressBar::hidden();
		ProgressBar { bar: Arc::new(bar) }
	}

	pub fn init(&self, message: &str, max_value: u64) {
		self.bar.set_length(max_value);
		self.bar.enable_steady_tick(Duration::from_millis(10000));
		self.bar.set_message(message.to_string());
		self.bar.set_style(
			ProgressStyle::default_bar()
				.template("{msg}▕{wide_bar}▏{pos}/{len} ({percent:>3}%) {per_sec:>5} {eta:>5}")
				.unwrap()
				.progress_chars("█▉▊▋▌▍▎▏  "),
		);
	}

	pub fn set_position(&self, value: u64) {
		self.bar.set_position(value);
	}

	pub fn set_max_value(&self, value: u64) {
		self.bar.set_length(value);
	}

	pub fn inc(&self, value: u64) {
		self.bar.inc(value);
	}

	pub fn finish(&self) {
		self.bar.finish();
	}

	pub fn remove(&self) {
		self.bar.finish_and_clear();
	}
}

impl Default for ProgressBar {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_bar_new() {
		let progress = ProgressBar::new();
		assert_eq!(progress.bar.length().unwrap_or(0), 0);
		assert_eq!(progress.bar.position(), 0);
	}

	#[test]
	fn test_bar_init() {
		let progress = ProgressBar::new();
		progress.init("Test", 100);
		assert_eq!(progress.bar.length().unwrap(), 100);
		assert_eq!(progress.bar.message(), "Test");
	}

	#[test]
	fn test_bar_set_position() {
		let progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(50);
		assert_eq!(progress.bar.position(), 50);
	}

	#[test]
	fn test_bar_inc() {
		let progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(10);
		progress.inc(20);
		assert_eq!(progress.bar.position(), 30);
	}

	#[test]
	fn test_bar_finish() {
		let progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(50);
		progress.finish();
		assert_eq!(progress.bar.position(), 100);
	}

	#[test]
	fn test_bar_remove() {
		let progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.remove();
		assert_eq!(progress.bar.position(), 100);
	}
}
