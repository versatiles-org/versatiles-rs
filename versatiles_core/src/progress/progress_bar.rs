//! Lightweight terminal progress bar without external dependencies.
//!
//! Features:
//! - message
//! - sub-character precision bar (7 partial block steps)
//! - pos/len
//! - percentage
//! - speed (items/sec)
//! - ETA

use std::cmp::min;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Inner {
	message: String,
	len: u64,
	pos: u64,
	start: Instant,
	finished: bool,
	last_draw: Instant,
}

impl Inner {
	fn redraw(&mut self) {
		if self.last_draw.elapsed() < Duration::from_secs(1) && !self.finished {
			return;
		}
		self.last_draw = Instant::now();

		let len = self.len.max(1); // avoid div by zero
		let pos = self.pos.min(len);
		let msg = &self.message;
		let elapsed = self.start.elapsed();
		let per_sec = if elapsed.as_secs_f64() > 0.0 {
			pos as f64 / elapsed.as_secs_f64()
		} else {
			0.0
		};
		let eta_secs = if per_sec > 0.0 {
			((len - pos) as f64 / per_sec).max(0.0)
		} else {
			0.0
		};

		// Compose the dynamic bar with sub-character precision.
		let (bar_str, bar_width) = make_bar(pos, len, available_bar_width(msg, pos, len, per_sec, eta_secs));

		let percent = (pos as f64 * 100.0 / len as f64).floor() as u64;
		let per_sec_str = format_rate(per_sec);
		let eta_str = format_eta(Duration::from_secs_f64(eta_secs));

		let mut line = String::new();
		let _ = write!(
			&mut line,
			"{}▕{}▏{}/{} ({:>3}%) {:>5} {:>5}",
			msg, bar_str, pos, len, percent, per_sec_str, eta_str
		);

		// Render to stderr with carriage return and clear line
		let mut stderr = io::stderr();
		let _ = write!(stderr, "\r\x1b[2K{}", line);
		let _ = stderr.flush();
		let _ = bar_width; // keep for symmetry and potential future use
	}
}

impl Default for Inner {
	fn default() -> Self {
		Inner {
			message: String::new(),
			len: 0,
			pos: 0,
			start: Instant::now(),
			finished: false,
			last_draw: Instant::now(),
		}
	}
}

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
		let _ = io::stderr().write_all(b"\n");
		let _ = io::stderr().flush();
	}

	/// Remove the bar line from the terminal.
	pub fn remove(&self) {
		let mutex = self.inner.clone();
		let mut inner = mutex.lock().unwrap();
		inner.pos = inner.len; // Semantics similar to previous tests
		inner.finished = true;
		drop(inner);
		// Clear current line
		let _ = io::stderr().write_all(b"\r\x1b[2K");
		let _ = io::stderr().flush();
	}
}

// Determine terminal width (rough heuristic: prefer $COLUMNS; fallback 80)
fn terminal_width() -> usize {
	if let Some((width, _)) = terminal_size::terminal_size() {
		return width.0.max(10) as usize;
	}
	80
}

// Compute how many characters are available for the bar itself,
// given the static decorations and metadata.
fn available_bar_width(msg: &str, pos: u64, len: u64, per_sec: f64, eta_secs: f64) -> usize {
	// We render: "{msg}▕{bar}▏{pos}/{len} ({pct}%) {per_sec} {eta}"
	// Estimate right side length (not including bar itself)
	let percent = (pos as f64 * 100.0 / len.max(1) as f64).floor() as u64;
	let per_sec_str = format_rate(per_sec);
	let eta_str = format_eta(Duration::from_secs_f64(eta_secs));

	// Static glyphs around the bar occupy 2 chars (▕ and ▏) plus spaces and fixed text
	let right = format!("▏{}/{} ({:>3}%) {:>5} {:>7}", pos, len, percent, per_sec_str, eta_str);
	let total_width = terminal_width();
	let taken = msg.chars().count() + right.chars().count();
	let min_bar = 10usize; // ensure a usable minimum width
	if total_width > taken + 2 + min_bar {
		total_width - taken - 2
	} else {
		min_bar
	}
}

fn make_bar(pos: u64, len: u64, width: usize) -> (String, usize) {
	let width = width.max(1);
	let frac = (pos as f64 / len.max(1) as f64).clamp(0.0, 1.0);
	let exact = frac * (width as f64);
	let whole = exact.floor() as usize;
	let rem = exact - whole as f64;

	// 7 partial steps + space (so 8 levels).
	// Highest density first to match original visuals.
	let partials = ["█", "▉", "▊", "▋", "▌", "▍", "▎", "▏"]; // last is thinnest

	let mut s = String::with_capacity(width);
	// Full cells
	for _ in 0..whole.min(width) {
		s.push('█');
	}
	if whole < width {
		// pick partial if there's any remainder
		let idx = (rem * 8.0).floor() as usize; // 0..=7
		if idx > 0 {
			s.push_str(partials[idx.min(7)]);
		} else {
			s.push(' ');
		}
		// pad rest with spaces
		let filled = whole + 1;
		for _ in filled..width {
			s.push(' ');
		}
	}
	(s, width)
}

fn format_rate(per_sec: f64) -> String {
	if per_sec.is_finite() {
		human_number(per_sec) + "/s"
	} else {
		"--/s".to_string()
	}
}

fn human_number(v: f64) -> String {
	let abs = v.abs();
	if abs >= 1_000_000_000.0 {
		format!("{:.1}G", v / 1_000_000_000.0)
	} else if abs >= 1_000_000.0 {
		format!("{:.1}M", v / 1_000_000.0)
	} else if abs >= 1_000.0 {
		format!("{:.1}k", v / 1_000.0)
	} else {
		format!("{:.0}", v)
	}
}

fn format_eta(d: Duration) -> String {
	let total = d.as_secs();
	let days = total / 86_400; // 24*3600
	let hours = (total % 86_400) / 3_600;
	let minutes = (total % 3_600) / 60;
	let seconds = total % 60;

	if total < 60 {
		// Seconds only: e.g. "45s"
		format!("{}s", seconds)
	} else if total < 3_600 {
		// Minutes:Seconds: e.g. "12:34"
		format!("{:02}:{:02}", minutes, seconds)
	} else if total < 86_400 {
		// Hours:Minutes:Seconds: e.g. "3:05:42"
		format!("{}:{:02}:{:02}", hours, minutes, seconds)
	} else {
		// Days and hours: e.g. "2d03h"
		format!("{}d{:02}h", days, hours)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

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

	#[rstest]
	#[case(0.0, "0/s")]
	#[case(1.0, "1/s")]
	#[case(999.0, "999/s")]
	#[case(1000.0, "1.0k/s")]
	#[case(1234.0, "1.2k/s")]
	#[case(999_900.0, "999.9k/s")]
	#[case(1_000_000.0, "1.0M/s")]
	#[case(f64::INFINITY, "--/s")]
	#[case(f64::NAN, "--/s")]
	fn test_format_rate(#[case] input: f64, #[case] expected: &str) {
		assert_eq!(format_rate(input), expected);
	}

	#[rstest]
	#[case(0.0, "0")]
	#[case(1.0, "1")]
	#[case(999.4, "999")]
	#[case(1_000.0, "1.0k")]
	#[case(12_345.0, "12.3k")]
	#[case(1_000_000.0, "1.0M")]
	#[case(1_500_000_000.0, "1.5G")]
	#[case(-1_500.0, "-1.5k")]
	fn test_human_number(#[case] input: f64, #[case] expected: &str) {
		assert_eq!(human_number(input), expected);
	}

	#[rstest]
	#[case(45, "45s")]
	#[case(59, "59s")]
	#[case(60, "01:00")]
	#[case(65, "01:05")]
	#[case(3_599, "59:59")]
	#[case(3_600, "1:00:00")]
	#[case(11_142, "3:05:42")]
	#[case(86_400, "1d00h")]
	#[case(189_300, "2d04h")]
	fn test_format_eta(#[case] secs: u64, #[case] expected: &str) {
		assert_eq!(format_eta(Duration::from_secs(secs)), expected);
	}
}
