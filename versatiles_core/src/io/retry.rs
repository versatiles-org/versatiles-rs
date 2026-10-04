//! Shared retry policy for network readers and writers (HTTP + SFTP).
//!
//! All network I/O sites (HTTP reads, SFTP reads, SFTP writes, the streaming
//! SFTP writer) use a single, process-wide [`RetryPolicy`] so they behave
//! consistently and can be tuned from the environment. The policy implements
//! capped exponential backoff with random jitter.
//!
//! Jitter matters for throughput jobs: when many ranges are read/written
//! concurrently and an origin briefly fails, un-jittered backoff makes every
//! in-flight request retry in lockstep and re-overload the origin. Jitter
//! spreads the retries out.
//!
//! # Configuration (environment variables)
//!
//! - `VERSATILES_NET_MAX_RETRIES` — retries after the first attempt (default `32`)
//! - `VERSATILES_NET_RETRY_BASE_MS` — first backoff in ms, doubles each retry (default `1000`)
//! - `VERSATILES_NET_RETRY_MAX_MS` — cap for a single backoff in ms (default `60000`)
//!
//! With the defaults a single read/write tolerates roughly 25–30 minutes of
//! continuous failure before giving up — chosen so a long, unattended,
//! planet-scale transfer survives a storage-box or CDN outage rather than
//! throwing away hours of work.

use std::{
	sync::{
		LazyLock,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

// Production defaults: ~25–30 min total tolerance per operation.
#[cfg(not(test))]
const DEFAULT_MAX_RETRIES: u32 = 32;
#[cfg(not(test))]
const DEFAULT_BASE_MS: u64 = 1_000;
#[cfg(not(test))]
const DEFAULT_MAX_MS: u64 = 60_000;
#[cfg(not(test))]
const DEFAULT_JITTER: f64 = 0.2;

// Test defaults: keep the retry-path tests fast and deterministic while still
// exercising the loops (3 attempts, sub-millisecond waits, no jitter).
#[cfg(test)]
const DEFAULT_MAX_RETRIES: u32 = 2;
#[cfg(test)]
const DEFAULT_BASE_MS: u64 = 1;
#[cfg(test)]
const DEFAULT_MAX_MS: u64 = 4;
#[cfg(test)]
const DEFAULT_JITTER: f64 = 0.0;

/// Capped-exponential-backoff-with-jitter retry policy shared by all network
/// readers and writers.
#[derive(Debug, Clone)]
pub(crate) struct RetryPolicy {
	/// Number of retries *after* the initial attempt (total attempts = `max_retries + 1`).
	pub max_retries: u32,
	/// Backoff before the first retry; doubles each subsequent retry up to `max_backoff`.
	pub base: Duration,
	/// Upper bound for a single backoff wait.
	pub max_backoff: Duration,
	/// Fractional jitter in `0.0..=1.0`; each wait is scaled by a random factor in
	/// `[1 - jitter, 1 + jitter]`.
	pub jitter: f64,
}

impl RetryPolicy {
	/// Backoff duration before retry number `retry` (`0` = first retry).
	#[must_use]
	pub fn backoff(&self, retry: u32) -> Duration {
		let base_ms = u64::try_from(self.base.as_millis()).unwrap_or(u64::MAX);
		let cap_ms = u64::try_from(self.max_backoff.as_millis()).unwrap_or(u64::MAX);
		// base * 2^retry, saturating, then clamped to the cap.
		let factor = 1u64.checked_shl(retry).unwrap_or(u64::MAX);
		let raw_ms = base_ms.saturating_mul(factor).min(cap_ms);
		Self::apply_jitter(Duration::from_millis(raw_ms), self.jitter)
	}

	fn apply_jitter(d: Duration, jitter: f64) -> Duration {
		if jitter <= 0.0 {
			return d;
		}
		// Random factor in [1 - jitter, 1 + jitter], never negative.
		let r = next_rand_f64() * 2.0 - 1.0;
		let scale = (1.0 + jitter * r).max(0.0);
		d.mul_f64(scale)
	}
}

/// The process-wide retry policy, configured from the environment on first use.
#[must_use]
pub(crate) fn policy() -> &'static RetryPolicy {
	static POLICY: LazyLock<RetryPolicy> = LazyLock::new(|| RetryPolicy {
		max_retries: env_u32("VERSATILES_NET_MAX_RETRIES", DEFAULT_MAX_RETRIES),
		base: Duration::from_millis(env_u64("VERSATILES_NET_RETRY_BASE_MS", DEFAULT_BASE_MS)),
		max_backoff: Duration::from_millis(env_u64("VERSATILES_NET_RETRY_MAX_MS", DEFAULT_MAX_MS)),
		jitter: DEFAULT_JITTER,
	});
	&POLICY
}

/// Parse a `u32` environment variable, warning and falling back on invalid input.
#[must_use]
pub(crate) fn env_u32(name: &str, default: u32) -> u32 {
	match std::env::var(name) {
		Ok(v) => v.trim().parse().unwrap_or_else(|_| {
			log::warn!("invalid value for {name}: {v:?}, using default {default}");
			default
		}),
		Err(_) => default,
	}
}

/// Parse a `u64` environment variable, warning and falling back on invalid input.
#[must_use]
pub(crate) fn env_u64(name: &str, default: u64) -> u64 {
	match std::env::var(name) {
		Ok(v) => v.trim().parse().unwrap_or_else(|_| {
			log::warn!("invalid value for {name}: {v:?}, using default {default}");
			default
		}),
		Err(_) => default,
	}
}

/// Cheap, lock-free pseudo-random `f64` in `[0, 1)` for jitter.
///
/// Quality is irrelevant here — it only needs to decorrelate concurrent
/// retriers — so this avoids pulling in an RNG dependency. A single shared
/// SplitMix64 state, seeded once from the wall clock, is advanced per call.
fn next_rand_f64() -> f64 {
	static STATE: LazyLock<AtomicU64> = LazyLock::new(|| {
		let seed = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(0x9E37_79B9_7F4A_7C15, |d| {
				// Mix seconds and sub-second nanos without a lossy u128 cast.
				d.as_secs()
					.wrapping_mul(1_000_000_000)
					.wrapping_add(u64::from(d.subsec_nanos()))
			}) | 1;
		AtomicU64::new(seed)
	});

	let prev = STATE.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
	let mut z = prev.wrapping_add(0x9E37_79B9_7F4A_7C15);
	z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
	z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
	z ^= z >> 31;
	// Top 53 bits → uniform f64 in [0, 1).
	(z >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn backoff_is_capped_exponential() {
		let p = RetryPolicy {
			max_retries: 10,
			base: Duration::from_millis(1000),
			max_backoff: Duration::from_millis(8000),
			jitter: 0.0,
		};
		assert_eq!(p.backoff(0), Duration::from_millis(1000));
		assert_eq!(p.backoff(1), Duration::from_millis(2000));
		assert_eq!(p.backoff(2), Duration::from_millis(4000));
		assert_eq!(p.backoff(3), Duration::from_millis(8000));
		// Capped from here on.
		assert_eq!(p.backoff(4), Duration::from_millis(8000));
		assert_eq!(p.backoff(40), Duration::from_millis(8000));
		// Very large retry index must not panic (shift overflow guarded).
		assert_eq!(p.backoff(1000), Duration::from_millis(8000));
	}

	#[test]
	fn jitter_stays_within_bounds() {
		let p = RetryPolicy {
			max_retries: 5,
			base: Duration::from_millis(1000),
			max_backoff: Duration::from_millis(60_000),
			jitter: 0.2,
		};
		for _ in 0..1000 {
			let d = p.backoff(2).as_millis() as f64; // raw 4000ms
			assert!((3200.0..=4800.0).contains(&d), "jittered backoff out of bounds: {d}");
		}
	}

	#[test]
	fn rand_is_in_unit_interval() {
		for _ in 0..1000 {
			let r = next_rand_f64();
			assert!((0.0..1.0).contains(&r), "rand out of range: {r}");
		}
	}

	#[test]
	fn env_helpers_fall_back_on_invalid() {
		// A name that is (almost certainly) unset returns the default.
		assert_eq!(env_u32("VERSATILES_TEST_DEFINITELY_UNSET_XYZ", 7), 7);
		assert_eq!(env_u64("VERSATILES_TEST_DEFINITELY_UNSET_XYZ", 9), 9);
	}

	#[test]
	fn global_policy_uses_test_defaults() {
		let p = policy();
		assert_eq!(p.max_retries, DEFAULT_MAX_RETRIES);
		assert_eq!(p.jitter, 0.0);
	}
}
