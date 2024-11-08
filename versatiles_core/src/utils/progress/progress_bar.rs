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
//! use versatiles::utils::progress::get_progress_bar;
//!
//! let mut progress = get_progress_bar("Processing", 100);
//! progress.set_position(50);
//! progress.inc(10);
//! progress.finish();
//! ```

use super::ProgressTrait;
use indicatif::{ProgressBar as IndicatifProgressBar, ProgressStyle};
use std::{
	sync::{Arc, Mutex},
	time::Duration,
};

/// A struct that represents a progress bar.
pub struct ProgressBar {
	bar: IndicatifProgressBar,
}

impl ProgressTrait for ProgressBar {
	fn new() -> Self {
		ProgressBar {
			bar: IndicatifProgressBar::new(0),
		}
	}

	fn init(&mut self, message: &str, max_value: u64) {
		let p = &mut self.bar;
		p.set_length(max_value);
		p.enable_steady_tick(Duration::from_millis(250));
		p.set_message(message.to_string());
		p.set_style(
			ProgressStyle::default_bar()
				.template("{msg}▕{wide_bar}▏{pos}/{len} ({percent}%) {per_sec} {eta_precise}")
				.unwrap()
				.progress_chars("█▉▊▋▌▍▎▏  "),
		);
	}

	fn set_position(&mut self, value: u64) {
		self.bar.set_position(value);
	}

	fn set_max_value(&mut self, value: u64) {
		self.bar.set_max_value(value);
	}

	fn inc(&mut self, value: u64) {
		self.bar.inc(value);
	}

	fn finish(&mut self) {
		self.bar.finish();
	}

	fn remove(&mut self) {
		self.bar.finish_and_clear();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	#[test]
	fn test_bar_new() {
		let progress = ProgressBar::new();
		assert_eq!(progress.bar.length().unwrap(), 0);
		assert_eq!(progress.bar.position(), 0);
	}

	#[test]
	fn test_bar_init() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		assert_eq!(progress.bar.length().unwrap(), 100);
		assert_eq!(progress.bar.message(), "Test");
	}

	#[test]
	fn test_bar_set_position() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(50);
		assert_eq!(progress.bar.position(), 50);
	}

	#[test]
	fn test_bar_inc() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(10);
		progress.inc(20);
		assert_eq!(progress.bar.position(), 30);
	}

	#[test]
	fn test_bar_finish() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(50);
		progress.finish();
		assert_eq!(progress.bar.position(), 100);
	}

	#[test]
	fn test_bar_remove() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.remove();
		assert_eq!(progress.bar.position(), 100);
	}
}
