use crate::{EventBus, ProgressId, ProgressState};
use parking_lot::Mutex;
use std::{
	sync::{
		Arc, Once,
		atomic::{AtomicBool, Ordering},
	},
	time::{Duration, Instant},
};
use versatiles_core::utils::float_to_int;

/// Set to `true` once we detect we are running inside a tmux session.
/// Written once in `install_osc_reset_hooks`; read (with Relaxed ordering)
/// from both normal code and the async-signal-safe signal handler.
static IN_TMUX: AtomicBool = AtomicBool::new(false);

/// Return `true` when the process is running inside a tmux session.
#[inline]
fn in_tmux() -> bool {
	IN_TMUX.load(Ordering::Relaxed)
}

/// Wrap an OSC sequence for the current terminal environment.
///
/// When inside tmux, the terminal multiplexer intercepts escape sequences
/// before they reach the outer terminal. The DCS passthrough prefix
/// `\x1bPtmux;\x1b` … `\x1b\\` tells tmux to forward the inner sequence
/// verbatim. Every `ESC` (`\x1b`) inside the payload must be doubled so
/// tmux does not misinterpret it as the end of the DCS string.
fn osc_wrap(seq: &str) -> String {
	if in_tmux() {
		// Double every ESC inside the payload, then wrap in DCS passthrough.
		let escaped = seq.replace('\x1b', "\x1b\x1b");
		format!("\x1bPtmux;\x1b{escaped}\x1b\\")
	} else {
		seq.to_string()
	}
}

/// Write the OSC 9;4 "clear progress" escape sequence to stderr.
///
/// Wraps the sequence for tmux when necessary.
fn osc_reset() {
	use std::io::Write;
	let seq = osc_wrap("\x1b]9;4;0;\x07");
	let _ = write!(std::io::stderr(), "{seq}");
	let _ = std::io::stderr().flush();
}

/// Install the panic hook and (on Unix) signal handlers that reset the
/// OSC progress indicator before the process terminates abnormally.
///
/// Safe to call multiple times — the setup runs exactly once.
/// Note: SIGKILL cannot be caught by any process; only SIGTERM and SIGINT
/// (Ctrl+C) are covered here.
fn install_osc_reset_hooks() {
	static ONCE: Once = Once::new();
	ONCE.call_once(|| {
		// Detect tmux once and cache the result in a static AtomicBool so
		// the signal handler (which must be allocation-free) can read it.
		if std::env::var_os("TMUX").is_some() {
			IN_TMUX.store(true, Ordering::Relaxed);
		}

		// Chain with any existing panic hook so existing behaviour is preserved.
		let prev = std::panic::take_hook();
		std::panic::set_hook(Box::new(move |info| {
			osc_reset();
			prev(info);
		}));

		// On Unix, install signal handlers for SIGTERM and SIGINT.
		// The handler writes the OSC reset directly via the async-signal-safe
		// `write(2)` syscall, then restores the default handler and re-raises
		// the signal so the process exits with the correct status/core dump.
		#[cfg(unix)]
		{
			// SAFETY: signal handlers must be async-signal-safe.  We only call
			// `libc::write` (which is async-signal-safe) and `AtomicBool::load`
			// (a plain memory load), then restore the default handler and
			// re-raise.  No Rust allocations or locks are used.
			unsafe extern "C" fn handle_signal(sig: libc::c_int) {
				// Static byte sequences; choose at runtime based on IN_TMUX.
				// Both sequences terminate the OSC 9;4 progress indicator.
				const PLAIN: &[u8] = b"\x1b]9;4;0;\x07";
				// DCS passthrough: ESC P tmux; ESC ESC ] 9;4;0; BEL ESC \
				const TMUX: &[u8] = b"\x1bPtmux;\x1b\x1b]9;4;0;\x07\x1b\\";
				let seq = if IN_TMUX.load(Ordering::Relaxed) { TMUX } else { PLAIN };
				// SAFETY: fd 2 is always open; seq is a valid byte slice.
				unsafe {
					libc::write(2, seq.as_ptr().cast(), seq.len());
					libc::signal(sig, libc::SIG_DFL);
					libc::raise(sig);
				}
			}
			unsafe {
				libc::signal(libc::SIGTERM, handle_signal as *const () as libc::sighandler_t);
				libc::signal(libc::SIGINT, handle_signal as *const () as libc::sighandler_t);
			}
		}
	});
}

/// Handle for tracking progress of an operation
///
/// Progress handles can be cloned and shared across threads. All clones
/// share the same underlying state and emit events to the same event bus.
#[derive(Clone)]
pub struct ProgressHandle {
	state: Arc<Mutex<ProgressState>>,
	event_bus: EventBus,
	silent: bool,
}

impl ProgressHandle {
	#[must_use]
	pub fn new(id: ProgressId, message: String, total: u64, event_bus: EventBus, silent: bool) -> Self {
		if !silent {
			install_osc_reset_hooks();
		}
		let start = Instant::now();
		let handle = Self {
			state: Arc::new(Mutex::new(ProgressState {
				id,
				message,
				position: 0,
				total,
				start,
				next_draw: start,
				next_emit: start,
				finished: false,
			})),
			event_bus,
			silent,
		};

		// Draw and emit initial progress event
		handle.redraw(&mut handle.state.lock());
		handle.emit_update();
		handle
	}

	/// Set absolute position
	///
	/// The position will be clamped to the maximum value (total).
	pub fn set_position(&self, position: u64) {
		let mut state = self.state.lock();
		state.position = position.min(state.total);
		self.redraw(&mut state);
		drop(state);
		self.emit_update();
	}

	/// Increment position by delta
	///
	/// The position will be clamped to the maximum value (total).
	pub fn inc(&self, delta: u64) {
		let mut state = self.state.lock();
		state.position = state.position.saturating_add(delta).min(state.total);
		self.redraw(&mut state);
		drop(state);
		self.emit_update();
	}

	/// Set maximum value (total)
	///
	/// If the current position exceeds the new total, it will be clamped.
	pub fn set_max_value(&self, total: u64) {
		let mut state = self.state.lock();
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
		let mut state = self.state.lock();
		state.position = state.total;
		state.finished = true;
		self.redraw(&mut state);
		drop(state);
		self.emit_update();
	}

	/// Get the progress ID
	#[must_use]
	pub fn id(&self) -> ProgressId {
		self.state.lock().id.clone()
	}

	/// Emit a progress update event (throttled to 10 per second)
	fn emit_update(&self) {
		let mut state = self.state.lock();
		let now = Instant::now();

		// Emit if:
		// 1. Progress is finished (always emit final state)
		// 2. Enough time has passed (100ms = 10 updates per second)
		if state.finished || now >= state.next_emit {
			// Update next emit time
			if !state.finished {
				state.next_emit = now + Duration::from_millis(10);
			}

			// Clone state and release lock before emitting
			let state_clone = state.clone();
			drop(state);

			self.event_bus.progress(state_clone);
		}
	}

	pub fn redraw(&self, state: &mut ProgressState) {
		if self.silent {
			return;
		}
		if state.next_draw > Instant::now() && !state.finished {
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

		let percent =
			float_to_int::<f64, u64>((pos as f64 * 100.0 / total as f64).floor()).expect("percent in 0..=100 fits in u64");
		let per_sec_str = format_rate(per_sec);
		let eta_str = format_eta(Duration::from_secs_f64(eta_secs));

		let get_line = |bar_str| format!("{msg}▕{bar_str}▏{pos}/{total} ({percent:>3}%) {per_sec_str:>5} {eta_str:>5}");

		let available_bar_width = terminal_width() - get_line("").chars().count();
		let bar_str = make_bar(pos, total, available_bar_width);
		let line = get_line(&bar_str);

		use std::io::Write;
		let mut output = std::io::stderr();
		// stderr write errors are non-fatal: progress is best-effort UI.
		// If the user piped stderr into a closed reader (e.g. `head -1`),
		// dropping these writes is preferable to aborting the program.
		if state.finished {
			// Clear terminal progress indicator
			let osc = osc_wrap("\x1b]9;4;0;\x07");
			let _ = write!(output, "\r\x1b[2K{line}{osc}");
		} else {
			// Set terminal progress indicator (OSC 9;4)
			let osc = osc_wrap(&format!("\x1b]9;4;1;{percent}\x07"));
			let _ = write!(output, "\r\x1b[2K{line}{osc}");
		}
		let _ = output.flush();
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
	#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
	let whole = exact.floor() as usize;
	let rem = exact - whole as f64;

	// 8 partial steps from thinnest to fullest (index 0 = empty, 7 = nearly full).
	let partials = ["▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

	let mut s = String::with_capacity(width);
	// Full cells
	for _ in 0..whole.min(width) {
		s.push('█');
	}
	if whole < width {
		// pick partial if there's any remainder
		#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	#[test]
	fn test_progress_handle_new() {
		let event_bus = EventBus::new();
		let handle = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		assert_eq!(handle.id().0, 1);
	}

	#[test]
	fn test_progress_handle_set_position() {
		let event_bus = EventBus::new();
		let handle = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		handle.set_position(50);
		let state = handle.state.lock();
		assert_eq!(state.position, 50);
	}

	#[test]
	fn test_progress_handle_set_position_clamps_to_max() {
		let event_bus = EventBus::new();
		let handle = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		handle.set_position(150); // Exceeds total
		let state = handle.state.lock();
		assert_eq!(state.position, 100); // Should be clamped to total
	}

	#[test]
	fn test_progress_handle_inc() {
		let event_bus = EventBus::new();
		let handle = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		handle.inc(10);
		handle.inc(15);
		handle.inc(25);

		let state = handle.state.lock();
		assert_eq!(state.position, 50);
	}

	#[test]
	fn test_progress_handle_inc_saturates() {
		let event_bus = EventBus::new();
		let handle = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		handle.set_position(90);
		handle.inc(20); // Would go to 110, but should clamp to 100

		let state = handle.state.lock();
		assert_eq!(state.position, 100);
	}

	#[test]
	fn test_progress_handle_set_max_value() {
		let event_bus = EventBus::new();
		let handle = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		handle.set_max_value(200);
		let state = handle.state.lock();
		assert_eq!(state.total, 200);
	}

	#[test]
	fn test_progress_handle_set_max_value_clamps_position() {
		let event_bus = EventBus::new();
		let handle = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		handle.set_position(80);
		handle.set_max_value(50); // New max is less than current position

		let state = handle.state.lock();
		assert_eq!(state.total, 50);
		assert_eq!(state.position, 50); // Position should be clamped
	}

	#[test]
	fn test_progress_handle_finish() {
		let event_bus = EventBus::new();
		let handle = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		handle.set_position(50);
		handle.finish();

		let state = handle.state.lock();
		assert_eq!(state.position, 100);
		assert!(state.finished);
	}

	#[test]
	fn test_progress_handle_clone() {
		let event_bus = EventBus::new();
		let handle1 = ProgressHandle::new(crate::ProgressId(1), "Test".to_string(), 100, event_bus, true);

		handle1.set_position(50);

		let handle2 = handle1.clone();
		handle2.set_position(75);

		// Both handles share the same state - verify by checking separately
		{
			let state1 = handle1.state.lock();
			assert_eq!(state1.position, 75);
		}
		{
			let state2 = handle2.state.lock();
			assert_eq!(state2.position, 75);
		}
	}

	// Helper function tests
	#[test]
	fn test_format_rate() {
		assert_eq!(format_rate(0.0), "0/s");
		assert_eq!(format_rate(50.0), "50/s");
		assert_eq!(format_rate(1500.0), "1.5k/s");
		assert_eq!(format_rate(2_500_000.0), "2.5M/s");
		assert_eq!(format_rate(3_500_000_000.0), "3.5G/s");
		assert_eq!(format_rate(f64::INFINITY), "--/s");
		assert_eq!(format_rate(f64::NAN), "--/s");
	}

	#[test]
	fn test_human_number() {
		assert_eq!(human_number(0.0), "0");
		assert_eq!(human_number(5.0), "5");
		assert_eq!(human_number(999.0), "999");
		assert_eq!(human_number(1_000.0), "1.0k");
		assert_eq!(human_number(1_500.0), "1.5k");
		assert_eq!(human_number(1_000_000.0), "1.0M");
		assert_eq!(human_number(2_500_000.0), "2.5M");
		assert_eq!(human_number(1_000_000_000.0), "1.0G");
		assert_eq!(human_number(3_500_000_000.0), "3.5G");
		assert_eq!(human_number(-1_500.0), "-1.5k");
	}

	#[test]
	fn test_format_eta() {
		assert_eq!(format_eta(Duration::from_secs(0)), "0s");
		assert_eq!(format_eta(Duration::from_secs(45)), "45s");
		assert_eq!(format_eta(Duration::from_secs(59)), "59s");
		assert_eq!(format_eta(Duration::from_secs(60)), "01:00");
		assert_eq!(format_eta(Duration::from_secs(90)), "01:30");
		assert_eq!(format_eta(Duration::from_secs(754)), "12:34"); // 12 min 34 sec
		assert_eq!(format_eta(Duration::from_secs(3_599)), "59:59");
		assert_eq!(format_eta(Duration::from_secs(3_600)), "1:00:00"); // 1 hour
		assert_eq!(format_eta(Duration::from_secs(11_142)), "3:05:42"); // 3h 5m 42s
		assert_eq!(format_eta(Duration::from_secs(86_399)), "23:59:59");
		assert_eq!(format_eta(Duration::from_secs(86_400)), "1d00h"); // 1 day
		assert_eq!(format_eta(Duration::from_secs(97_200)), "1d03h"); // 1d 3h
		assert_eq!(format_eta(Duration::from_secs(183_600)), "2d03h"); // 2d 3h
	}

	#[test]
	fn test_make_bar_empty() {
		let bar = make_bar(0, 100, 10);
		assert_eq!(bar.chars().count(), 10);
		assert!(bar.starts_with(' '));
	}

	#[test]
	fn test_make_bar_full() {
		let bar = make_bar(100, 100, 10);
		assert_eq!(bar.chars().count(), 10);
		assert_eq!(bar, "██████████");
	}

	#[test]
	fn test_make_bar_half() {
		let bar = make_bar(50, 100, 10);
		assert_eq!(bar.chars().count(), 10);
		// Should have 5 full blocks
		let full_count = bar.chars().filter(|&c| c == '█').count();
		assert_eq!(full_count, 5);
	}

	#[test]
	fn test_make_bar_partial() {
		let bar = make_bar(25, 100, 10);
		assert_eq!(bar.chars().count(), 10);
		// Should have some full blocks and a partial
		let full_count = bar.chars().filter(|&c| c == '█').count();
		assert_eq!(full_count, 2); // 2.5 -> 2 full blocks + 1 partial
	}

	#[test]
	fn test_make_bar_minimum_width() {
		let bar = make_bar(50, 100, 0);
		assert_eq!(bar.chars().count(), 1); // width.max(1)
	}

	#[test]
	fn test_make_bar_zero_total() {
		// Should handle division by zero gracefully
		let bar = make_bar(0, 0, 10);
		assert_eq!(bar.chars().count(), 10);
	}

	#[test]
	fn test_terminal_width() {
		// Just verify it returns a reasonable value
		let width = terminal_width();
		assert!(width >= 10); // Minimum fallback is 10
	}
}
