//! Per-tile byte-size accounting for the read operations that synthesise
//! vector tiles (`from_geo`, `from_csv`).
//!
//! Vector tiles much larger than ~200 KB stop being usable in clients
//! (Mapbox / MapLibre raise warnings, parsers slow down, network round-trip
//! suffers). Anything past ~1 MB is essentially broken for downstream
//! consumption. Misconfiguration that causes such bloat (no property
//! pruning, no point reduction, max_zoom too low) is silently catastrophic
//! — the user only finds out when the tiles fail to render.
//!
//! [`TileSizeMonitor`] sits between the tile encoder and the consumer:
//!
//! - **Hard cap** ([`HARD_CAP_BYTES`], 1 MB) → return an error from `check`,
//!   aborting the conversion with a clear message including `(z, x, y)`.
//! - **Soft cap** ([`SOFT_CAP_BYTES`], 200 KB) → track silently; emit a
//!   single one-shot warning the first time it's hit so the user gets
//!   live feedback. Per-tile warnings would drown out the log on a
//!   multi-million-tile pyramid.
//! - **End-of-run summary** → on `Drop`, log total tile count, byte
//!   averages, and the top largest tiles so the user can see exactly
//!   where the problem is.

use anyhow::{Result, bail};
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use versatiles_core::{Blob, TileCoord};

/// Tiles below this size never trigger a warning.
pub const SOFT_CAP_BYTES: usize = 200 * 1024;

/// Tiles above this size cause `check` to return an error. ~1 MB is well
/// past the practical limit of Mapbox / MapLibre style clients and well into
/// "this feed is misconfigured" territory.
pub const HARD_CAP_BYTES: usize = 1024 * 1024;

/// How many of the largest tiles to remember and report at end-of-run.
const TOP_N: usize = 5;

/// Shared accounting state for one read operation. Cloning a
/// [`TileSizeMonitor`] is cheap (it's an `Arc`); each clone reports into the
/// same counters. The end-of-run summary fires when the last clone drops.
#[derive(Clone)]
pub struct TileSizeMonitor {
	inner: Arc<MonitorInner>,
}

struct MonitorInner {
	/// Identifier prefix for log lines, e.g. `"from_geo"`.
	label: &'static str,
	tile_count: AtomicU64,
	oversized_count: AtomicU64,
	total_bytes: AtomicU64,
	max_bytes: AtomicUsize,
	first_oversized_warned: AtomicBool,
	top: Mutex<Vec<(usize, TileCoord)>>,
}

impl TileSizeMonitor {
	#[must_use]
	pub fn new(label: &'static str) -> Self {
		Self {
			inner: Arc::new(MonitorInner {
				label,
				tile_count: AtomicU64::new(0),
				oversized_count: AtomicU64::new(0),
				total_bytes: AtomicU64::new(0),
				max_bytes: AtomicUsize::new(0),
				first_oversized_warned: AtomicBool::new(false),
				top: Mutex::new(Vec::with_capacity(TOP_N + 1)),
			}),
		}
	}

	/// Check the encoded blob's size against the soft and hard caps and
	/// update accounting. Errors when above [`HARD_CAP_BYTES`].
	pub fn check(&self, coord: TileCoord, blob: &Blob) -> Result<()> {
		#[allow(clippy::cast_possible_truncation)]
		let size = blob.len() as usize;
		if size > HARD_CAP_BYTES {
			bail!(
				"{}: tile {}/{}/{} is {} KB, exceeding the {} KB hard cap. \
				Likely causes: missing property pruning on a feature-heavy input, \
				`point_reduction='none'` on a dense point feed, or `max_zoom` set too low \
				so all features pile into a few tiles.",
				self.inner.label,
				coord.level,
				coord.x,
				coord.y,
				size / 1024,
				HARD_CAP_BYTES / 1024,
			);
		}

		let inner = &self.inner;
		inner.tile_count.fetch_add(1, Ordering::Relaxed);
		inner.total_bytes.fetch_add(size as u64, Ordering::Relaxed);
		inner.max_bytes.fetch_max(size, Ordering::Relaxed);

		if size > SOFT_CAP_BYTES {
			inner.oversized_count.fetch_add(1, Ordering::Relaxed);
			// One-shot live warning so the user knows during the build that
			// something is off. Subsequent oversized tiles roll up into the
			// end-of-run summary instead of spamming the log.
			if inner
				.first_oversized_warned
				.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
				.is_ok()
			{
				log::warn!(
					"{}: tile {}/{}/{} is {} KB (> {} KB soft cap). Further oversized tiles will be summarized at end of run.",
					inner.label,
					coord.level,
					coord.x,
					coord.y,
					size / 1024,
					SOFT_CAP_BYTES / 1024,
				);
			}
			let mut top = inner.top.lock().expect("poisoned tile-size monitor mutex");
			top.push((size, coord));
			top.sort_by_key(|entry| std::cmp::Reverse(entry.0));
			top.truncate(TOP_N);
		}
		Ok(())
	}
}

impl Drop for MonitorInner {
	fn drop(&mut self) {
		let count = *self.tile_count.get_mut();
		if count == 0 {
			return; // monitor was created but no tile ever flowed through; stay quiet.
		}
		let oversized = *self.oversized_count.get_mut();
		let total = *self.total_bytes.get_mut();
		let max = *self.max_bytes.get_mut();
		let avg = total / count.max(1);
		log::info!(
			"{}: emitted {count} tile(s); avg {} bytes, max {} bytes; {oversized} over {} KB",
			self.label,
			avg,
			max,
			SOFT_CAP_BYTES / 1024,
		);
		if oversized > 0 {
			let top = self.top.get_mut().expect("poisoned tile-size monitor mutex");
			if !top.is_empty() {
				let lines: Vec<String> = top
					.iter()
					.map(|(s, c)| format!("{}/{}/{} = {} KB", c.level, c.x, c.y, s / 1024))
					.collect();
				log::warn!(
					"{}: top {} oversized tile(s): [{}]. Consider trimming properties, raising `max_zoom`, or enabling stronger point reduction.",
					self.label,
					top.len(),
					lines.join(", "),
				);
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn under_soft_cap_passes_silently() {
		let monitor = TileSizeMonitor::new("test");
		let blob = Blob::from(vec![0u8; 1024]);
		monitor.check(TileCoord::new(0, 0, 0).unwrap(), &blob).unwrap();
	}

	#[test]
	fn over_soft_cap_passes_but_records() {
		let monitor = TileSizeMonitor::new("test");
		let blob = Blob::from(vec![0u8; SOFT_CAP_BYTES + 1024]);
		monitor.check(TileCoord::new(5, 1, 2).unwrap(), &blob).unwrap();
		// One tile, one oversized.
		assert_eq!(monitor.inner.tile_count.load(Ordering::Relaxed), 1);
		assert_eq!(monitor.inner.oversized_count.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn over_hard_cap_errors() {
		let monitor = TileSizeMonitor::new("test");
		let blob = Blob::from(vec![0u8; HARD_CAP_BYTES + 1]);
		let err = monitor.check(TileCoord::new(7, 3, 4).unwrap(), &blob).unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("hard cap"), "{msg}");
		assert!(msg.contains("7/3/4"), "{msg}");
	}

	#[test]
	fn top_n_keeps_largest() {
		let monitor = TileSizeMonitor::new("test");
		// Five tiles, ascending sizes — only the 5 largest stay (which is all).
		// Add a 6th and verify the smallest is dropped.
		let sizes_kb = [210, 220, 230, 240, 250, 260];
		for (i, kb) in sizes_kb.iter().enumerate() {
			let blob = Blob::from(vec![0u8; kb * 1024]);
			// z=4 has plenty of x slots; the exact coord doesn't matter for the test.
			let coord = TileCoord::new(4, u32::try_from(i).unwrap(), 0).unwrap();
			monitor.check(coord, &blob).unwrap();
		}
		let top = monitor.inner.top.lock().unwrap();
		assert_eq!(top.len(), TOP_N);
		// Largest first, smallest in the kept window is 220 KB.
		assert_eq!(top[0].0, 260 * 1024);
		assert_eq!(top.last().unwrap().0, 220 * 1024);
	}
}
