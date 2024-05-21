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
//! use versatiles::types::progress::{ProgressBar, ProgressTrait};
//!
//! let mut progress = ProgressBar::new();
//! progress.init("Processing", 100);
//! progress.set_position(50);
//! progress.inc(10);
//! progress.finish();
//! ```

use super::ProgressTrait;
use std::{
	io::Write,
	time::{Duration, SystemTime},
};
use terminal_size::terminal_size;

const TICKER_INTERVAL: Duration = Duration::from_millis(200);

/// A struct that represents a progress bar.
pub struct ProgressBar {
	/// The maximum value of the progress bar.
	max_value: u64,
	/// A message that describes the task being performed.
	message: String,
	/// The time at which the task was started.
	start_time: SystemTime,
	/// The time at which the progress bar was last updated.
	update_time: SystemTime,
	/// The current value of the progress bar.
	value: u64,
}

impl ProgressBar {
	/// Draws the progress bar in the terminal.
	fn draw(&mut self) {
		if self.max_value == 0 {
			return;
		}

		if SystemTime::now() < self.update_time {
			return;
		}

		let width = terminal_size().map_or(80, |s| s.0 .0) as i64;

		let duration = SystemTime::now().duration_since(self.start_time).unwrap();
		let progress = (self.value as f64 / self.max_value as f64).clamp(1e-6, 1.);
		let time_left = Duration::from_secs_f64(duration.as_secs_f64() / progress * (1.0 - progress));
		let speed = self.value as f64 / duration.as_secs_f64();

		let col1 = self.message.to_string();
		let col2 = format!(
			"{:>15}/{:<15}{:.2}%",
			format_integer(self.value),
			format_integer(self.max_value),
			progress * 100.0
		);
		let col3 = format!(
			"{}/s {:>15} {:>15}",
			format_float(speed),
			format_duration(duration),
			format_duration(time_left)
		);

		let length1 = col1.len() as i64;
		let length2 = col2.len() as i64;
		let length3 = col3.len() as i64;

		let space_len1 = 0.max((width - length2) / 2 - length1);
		let space_len2 = 0.max(width - (length1 + space_len1 + length2 + length3));

		let space1 = " ".repeat(space_len1 as usize);
		let space2 = " ".repeat(space_len2 as usize);

		let line = format!("\r{col1}{space1}{col2}{space2}{col3}");
		let pos = (line.len() as f64 * progress).round() as usize;

		eprint!("\r\x1B[7m{}\x1B[0m{}", &line[0..pos], &line[pos..]);
		std::io::stdout().flush().unwrap();

		self.update_time = self.update_time.checked_add(TICKER_INTERVAL).unwrap();
	}
}

impl ProgressTrait for ProgressBar {
	fn new() -> Self {
		let start_time = SystemTime::now();
		ProgressBar {
			max_value: 0,
			message: String::from(""),
			start_time,
			value: 0,
			update_time: start_time.checked_add(TICKER_INTERVAL).unwrap(),
		}
	}

	fn init(&mut self, message: &str, max_value: u64) {
		self.message = message.to_string();
		self.max_value = max_value;
		self.start_time = SystemTime::now();
		self.value = 0;
		self.draw();
	}

	/// Sets the position of the progress bar.
	///
	/// # Arguments
	///
	/// * `value`: The new position of the progress bar.
	fn set_position(&mut self, value: u64) {
		self.value = value;
		self.draw();
	}

	/// Increases the value of the progress bar by a given amount.
	///
	/// # Arguments
	///
	/// * `value`: The amount by which to increase the progress bar.
	fn inc(&mut self, value: u64) {
		self.value += value;
		self.draw();
	}

	/// Finishes the progress bar and sets its value to the maximum.
	fn finish(&mut self) {
		self.value = self.max_value;
		self.update_time = self.start_time;
		self.draw();
		eprintln!();
	}

	/// Removes the progress bar from the terminal.
	fn remove(&mut self) {
		eprint!("\r\x1B[2K");
		std::io::stdout().flush().unwrap();
	}
}

/// Converts a `Duration` to a formatted string in the format "HH:MM:SS" or "Dd HH:MM:SS"
/// depending on the duration's length.
///
/// # Arguments
///
/// * `duration` - A `Duration` object representing the length of time to format.
///
/// # Returns
///
/// A string representing the formatted duration.
fn format_duration(duration: Duration) -> String {
	let mut t = duration.as_secs();
	let seconds = t % 60;
	t /= 60;
	let minutes = t % 60;
	t /= 60;
	let hours = t % 24;
	t /= 24;
	if t > 0 {
		let days = t;
		format!("{days}d {hours:02}:{minutes:02}:{seconds:02}")
	} else {
		format!("{hours:02}:{minutes:02}:{seconds:02}")
	}
}

/// Formats a large integer value by adding an apostrophe every three digits.
///
/// # Arguments
///
/// * `value` - An integer to format.
///
/// # Returns
///
/// A string representing the formatted integer.
fn format_integer(value: u64) -> String {
	value
		.to_string()
		.as_bytes()
		.rchunks(3)
		.rev()
		.map(std::str::from_utf8)
		.collect::<Result<Vec<&str>, _>>()
		.unwrap()
		.join("'")
}

/// Formats a floating-point value to a string with varying decimal places based on its magnitude.
///
/// # Arguments
///
/// * `value` - A floating-point value to format.
///
/// # Returns
///
/// A string representing the formatted floating-point value.
fn format_float(mut value: f64) -> String {
	value *= 1.0000000000001;
	if value > 1000.0 {
		format_integer(value.round() as u64)
	} else if value > 100.0 {
		format!("{:.1}", value)
	} else if value > 10.0 {
		format!("{:.2}", value)
	} else {
		format!("{:.3}", value)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_format_integer() {
		assert_eq!(format_integer(1234567890), "1'234'567'890");
		assert_eq!(format_integer(123456789), "123'456'789");
		assert_eq!(format_integer(1000), "1'000");
		assert_eq!(format_integer(100), "100");
	}

	#[test]
	fn test_format_duration() {
		assert_eq!(format_duration(Duration::from_secs(1)), "00:00:01");
		assert_eq!(format_duration(Duration::from_secs(60)), "00:01:00");
		assert_eq!(format_duration(Duration::from_secs(60 * 60)), "01:00:00");
		assert_eq!(format_duration(Duration::from_secs(60 * 60 * 24)), "1d 00:00:00");
		assert_eq!(format_duration(Duration::from_secs(60 * 60 * 24 + 3661)), "1d 01:01:01");
	}

	#[test]
	fn test_format_float() {
		assert_eq!(format_float(1234.5), "1'235");
		assert_eq!(format_float(123.45), "123.5");
		assert_eq!(format_float(12.345), "12.35");
		assert_eq!(format_float(1.2345), "1.235");
		assert_eq!(format_float(0.2345), "0.235");
	}

	#[test]
	fn test_progress_bar_new() {
		let progress = ProgressBar::new();
		assert_eq!(progress.max_value, 0);
		assert_eq!(progress.message, "");
		assert_eq!(progress.value, 0);
	}

	#[test]
	fn test_progress_bar_init() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		assert_eq!(progress.max_value, 100);
		assert_eq!(progress.message, "Test");
		assert_eq!(progress.value, 0);
	}

	#[test]
	fn test_progress_bar_set_position() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(50);
		assert_eq!(progress.value, 50);
	}

	#[test]
	fn test_progress_bar_inc() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(10);
		progress.inc(20);
		assert_eq!(progress.value, 30);
	}

	#[test]
	fn test_progress_bar_finish() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(50);
		progress.finish();
		assert_eq!(progress.value, 100);
	}

	#[test]
	fn test_progress_bar_remove() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.remove();
	}

	#[test]
	fn test_progress_bar_draw() {
		let mut progress = ProgressBar::new();
		progress.init("Test", 100);
		progress.set_position(50);
		progress.draw();
	}
}
