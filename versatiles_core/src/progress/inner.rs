//! Lightweight terminal inner bar without external dependencies.
//!
//! Features:
//! - message
//! - sub-character precision bar (7 partial block steps)
//! - pos/len
//! - percentage
//! - speed (items/sec)
//! - ETA

use std::time::{Duration, Instant};

pub struct Inner {
	pub message: String,
	pub len: u64,
	pub pos: u64,
	pub start: Instant,
	pub finished: bool,
	pub last_draw: Instant,
}

impl Inner {
	pub fn redraw(&mut self) {
		if self.last_draw.elapsed() < Duration::from_millis(500) && !self.finished {
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
		let eta_secs = if pos > 0 {
			elapsed.as_secs_f64() * ((len - pos) as f64 / (pos as f64)).max(0.0)
		} else {
			0.0
		};

		let percent = (pos as f64 * 100.0 / len as f64).floor() as u64;
		let per_sec_str = format_rate(per_sec);
		let eta_str = format_eta(Duration::from_secs_f64(eta_secs));

		let get_line = |bar_str| format!("{msg}▕{bar_str}▏{pos}/{len} ({percent:>3}%) {per_sec_str:>5} {eta_str:>5}");

		let available_bar_width = terminal_width() - get_line("").chars().count();
		let bar_str = make_bar(pos, len, available_bar_width);
		let line = get_line(&bar_str);

		// Render to stderr with carriage return and clear line
		self.write(&format!("\r\x1b[2K{line}"));
	}

	#[allow(unused_variables)]
	pub fn write(&mut self, line: &str) {
		#[cfg(not(any(test, feature = "test", not(feature = "cli"))))]
		{
			use std::io::Write;
			let mut output = std::io::stderr();
			write!(output, "{line}").unwrap();
			output.flush().unwrap();
		}
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

// Determine terminal width (rough heuristic: prefer $COLUMNS; fallback 80)
fn terminal_width() -> usize {
	if let Some((width, _)) = terminal_size::terminal_size() {
		return width.0.max(10) as usize;
	}
	80
}

fn make_bar(pos: u64, len: u64, width: usize) -> String {
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
	s
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
		format!("{v:.0}")
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
		format!("{seconds}s")
	} else if total < 3_600 {
		// Minutes:Seconds: e.g. "12:34"
		format!("{minutes:02}:{seconds:02}")
	} else if total < 86_400 {
		// Hours:Minutes:Seconds: e.g. "3:05:42"
		format!("{hours}:{minutes:02}:{seconds:02}")
	} else {
		// Days and hours: e.g. "2d03h"
		format!("{days}d{hours:02}h")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[test]
	fn test_default() {
		let inner = Inner::default();
		assert_eq!(inner.len, 0);
		assert_eq!(inner.pos, 0);
		assert_eq!(inner.message, "");
	}

	#[test]
	#[allow(clippy::field_reassign_with_default)]
	fn test_fields() {
		let mut inner = Inner::default();
		inner.len = 100;
		inner.pos = 50;
		inner.message = "Test".to_string();
		assert_eq!(inner.len, 100);
		assert_eq!(inner.pos, 50);
		assert_eq!(inner.message, "Test");
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
