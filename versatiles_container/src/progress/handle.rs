use crate::{EventBus, ProgressId, ProgressState};
use std::{
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

/// Handle for tracking progress of an operation
///
/// Progress handles can be cloned and shared across threads. All clones
/// share the same underlying state and emit events to the same event bus.
#[derive(Clone)]
pub struct ProgressHandle {
	state: Arc<Mutex<ProgressState>>,
	event_bus: EventBus,
	stderr: bool,
}

impl ProgressHandle {
	pub fn new(id: ProgressId, message: String, total: u64, event_bus: EventBus, stderr: bool) -> Self {
		let start = Instant::now();
		let handle = Self {
			state: Arc::new(Mutex::new(ProgressState {
				id,
				message,
				position: 0,
				total,
				start,
				next_draw: start,
				finished: false,
			})),
			event_bus,
			stderr,
		};

		// Emit initial progress event
		handle.emit_update();
		handle
	}

	/// Set absolute position
	///
	/// The position will be clamped to the maximum value (total).
	pub fn set_position(&self, position: u64) {
		let mut state = self.state.lock().unwrap();
		state.position = position.min(state.total);
		self.redraw(&mut state);
		drop(state);
		self.emit_update();
	}

	/// Increment position by delta
	///
	/// The position will be clamped to the maximum value (total).
	pub fn inc(&self, delta: u64) {
		let mut state = self.state.lock().unwrap();
		state.position = state.position.saturating_add(delta).min(state.total);
		drop(state);
		self.emit_update();
	}

	/// Set maximum value (total)
	///
	/// If the current position exceeds the new total, it will be clamped.
	pub fn set_max_value(&self, total: u64) {
		let mut state = self.state.lock().unwrap();
		state.total = total;
		if state.position > state.total {
			state.position = state.total;
		}
		drop(state);
		self.emit_update();
	}

	/// Mark progress as finished
	///
	/// Sets position to total and marks the progress as complete.
	pub fn finish(&self) {
		let mut state = self.state.lock().unwrap();
		state.position = state.total;
		state.finished = true;
		drop(state);
		self.emit_update();
	}

	/// Get the progress ID
	pub fn id(&self) -> ProgressId {
		self.state.lock().unwrap().id.clone()
	}

	/// Emit a progress update event
	fn emit_update(&self) {
		let state = self.state.lock().unwrap().clone();
		self.event_bus.progress(state);
	}

	pub fn redraw(&self, state: &mut ProgressState) {
		if !self.stderr {
			return;
		}
		if state.next_draw < Instant::now() && !state.finished {
			return;
		}
		state.next_draw = Instant::now() + Duration::from_millis(500);

		let total = state.total.max(1); // avoid div by zero
		let pos = state.position.min(total);
		let msg = &state.message;
		let elapsed = state.start.elapsed();
		let per_sec = if elapsed.as_secs_f64() > 0.0 {
			pos as f64 / elapsed.as_secs_f64()
		} else {
			0.0
		};
		let eta_secs = if pos > 0 {
			elapsed.as_secs_f64() * ((total - pos) as f64 / (pos as f64)).max(0.0)
		} else {
			0.0
		};

		let percent = (pos as f64 * 100.0 / total as f64).floor() as u64;
		let per_sec_str = format_rate(per_sec);
		let eta_str = format_eta(Duration::from_secs_f64(eta_secs));

		let get_line = |bar_str| format!("{msg}▕{bar_str}▏{pos}/{total} ({percent:>3}%) {per_sec_str:>5} {eta_str:>5}");

		let available_bar_width = terminal_width() - get_line("").chars().count();
		let bar_str = make_bar(pos, total, available_bar_width);
		let line = get_line(&bar_str);

		use std::io::Write;
		let mut output = std::io::stderr();
		write!(output, "\r\x1b[2K{line}").unwrap();
		output.flush().unwrap();
	}
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
