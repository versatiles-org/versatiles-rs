//! Process memory heartbeat.
//!
//! Periodically logs the process resident set size (RSS) during long-running
//! commands so memory growth is visible in the program's own output — in
//! particular, climbing RSS right before an out-of-memory kill, which the
//! process itself cannot report (a `SIGKILL` can't be caught or logged).
//!
//! Controlled by `VERSATILES_MEMORY_LOG_SECS` (default `60`; set to `0` to
//! disable). RSS is read from `/proc/self/status` and is therefore Linux-only;
//! on other platforms the heartbeat is a no-op.

use std::time::Duration;

use tokio::task::JoinHandle;

const DEFAULT_INTERVAL_SECS: u64 = 60;

/// Current resident set size of this process in bytes, if available.
///
/// Reads `VmRSS` from `/proc/self/status` (Linux only); returns `None` elsewhere
/// or if the value cannot be read.
#[must_use]
pub fn process_rss_bytes() -> Option<u64> {
	#[cfg(target_os = "linux")]
	{
		let status = std::fs::read_to_string("/proc/self/status").ok()?;
		for line in status.lines() {
			if let Some(rest) = line.strip_prefix("VmRSS:") {
				// e.g. "VmRSS:\t 1234567 kB"
				let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
				return Some(kb * 1024);
			}
		}
		None
	}
	#[cfg(not(target_os = "linux"))]
	{
		None
	}
}

/// Human-readable byte count for log messages.
fn format_bytes(n: u64) -> String {
	const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
	let mut value = n as f64;
	let mut unit = 0;
	while value >= 1024.0 && unit < UNITS.len() - 1 {
		value /= 1024.0;
		unit += 1;
	}
	format!("{value:.1} {}", UNITS[unit])
}

fn interval_secs() -> u64 {
	interval_from(std::env::var("VERSATILES_MEMORY_LOG_SECS").ok().as_deref())
}

/// The interval a value of `VERSATILES_MEMORY_LOG_SECS` asks for: the default when it is unset or
/// not a number.
///
/// Separate from reading the variable so that tests need not set it: tests run in parallel, and one
/// that sets a process-wide variable changes what every other test reads.
fn interval_from(value: Option<&str>) -> u64 {
	value
		.and_then(|s| s.trim().parse::<u64>().ok())
		.unwrap_or(DEFAULT_INTERVAL_SECS)
}

/// Stops the heartbeat task when dropped.
pub struct HeartbeatGuard(JoinHandle<()>);

impl Drop for HeartbeatGuard {
	fn drop(&mut self) {
		self.0.abort();
	}
}

/// Starts a background task that logs RSS (current and peak) at the configured
/// interval. Returns `None` — and logs nothing — when disabled (`secs == 0`) or
/// when RSS is unavailable on this platform.
///
/// Must be called from within a Tokio runtime; the returned guard aborts the task
/// when it goes out of scope.
#[must_use]
pub fn start() -> Option<HeartbeatGuard> {
	start_with(interval_secs())
}

/// [`start`] with the interval given rather than read from the environment.
fn start_with(secs: u64) -> Option<HeartbeatGuard> {
	if secs == 0 || process_rss_bytes().is_none() {
		return None;
	}

	let handle = tokio::spawn(async move {
		let mut interval = tokio::time::interval(Duration::from_secs(secs));
		let mut peak = 0u64;
		loop {
			interval.tick().await;
			if let Some(rss) = process_rss_bytes() {
				peak = peak.max(rss);
				log::debug!("memory: RSS {} (peak {})", format_bytes(rss), format_bytes(peak));
			}
		}
	});
	Some(HeartbeatGuard(handle))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_format_bytes() {
		assert_eq!(format_bytes(0), "0.0 B");
		assert_eq!(format_bytes(1536), "1.5 KiB");
		assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MiB");
		assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
	}

	#[test]
	fn test_interval_from_the_variable() {
		assert_eq!(interval_from(None), DEFAULT_INTERVAL_SECS);
		assert_eq!(interval_from(Some("0")), 0);
		assert_eq!(interval_from(Some(" 15 ")), 15);
		assert_eq!(interval_from(Some("soon")), DEFAULT_INTERVAL_SECS);
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn test_rss_available_on_linux() {
		let rss = process_rss_bytes().expect("RSS should be readable on Linux");
		assert!(rss > 0, "RSS should be positive");
	}

	#[tokio::test]
	async fn test_start_disabled_returns_none() {
		assert!(start_with(0).is_none(), "interval 0 must disable the heartbeat");
	}
}
