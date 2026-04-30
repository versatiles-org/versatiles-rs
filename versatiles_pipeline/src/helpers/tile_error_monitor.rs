//! Per-tile error accounting for the read operations that synthesise vector
//! tiles (`from_geo`, `from_csv`).
//!
//! [`TileStream::from_bbox_parallel`] takes a callback returning `Option<T>`,
//! so every error inside the tile-generation pipeline (decoding, compression,
//! size cap) is silently coerced to "no tile". On a multi-million-tile
//! pyramid that's catastrophic when a systemic bug makes every tile fail —
//! the user sees a clean exit and an empty container.
//!
//! [`TileErrorMonitor`] sits next to [`super::tile_size_monitor::TileSizeMonitor`]
//! and mirrors its pattern:
//!
//! - **First error per stage** → `log::warn!` so the user gets live feedback
//!   that something's wrong without waiting for the run to finish.
//! - **Subsequent errors** → `log::debug!` only, so the log doesn't drown
//!   when a single bad input triggers errors on every one of N tiles.
//! - **End-of-run summary on `Drop`** → total errors and first-error sample
//!   per stage, so even a fully-quiet run leaves a clear trail.

use anyhow::Error;
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicU64, Ordering},
};
use versatiles_core::TileCoord;

/// Logical pipeline stage where a per-tile error happened. Picking the stage
/// at the call site (instead of parsing it from the error message) keeps the
/// summary stable and groupable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TileErrorStage {
	/// `FeatureImport::get_tile` returned `Err` (e.g. encoding failure).
	Render,
	/// `Tile::from_vector` failed (should not happen unless the format is wrong).
	Wrap,
	/// `tile.change_compression` failed.
	Compress,
	/// `tile.as_blob` failed.
	Serialize,
	/// `TileSizeMonitor::check` returned `Err` (hard cap exceeded).
	OverHardCap,
}

impl TileErrorStage {
	const fn as_str(self) -> &'static str {
		match self {
			Self::Render => "render",
			Self::Wrap => "wrap",
			Self::Compress => "compress",
			Self::Serialize => "serialize",
			Self::OverHardCap => "over-hard-cap",
		}
	}
}

/// Shared error-accounting state for one read operation. Cloning is cheap
/// (it's an `Arc`); each clone reports into the same counters. The end-of-run
/// summary fires when the last clone drops.
#[derive(Clone)]
pub struct TileErrorMonitor {
	inner: Arc<MonitorInner>,
}

struct MonitorInner {
	/// Identifier prefix for log lines, e.g. `"from_geo"`.
	label: &'static str,
	/// One slot per [`TileErrorStage`], in declaration order.
	stages: [StageStats; 5],
}

struct StageStats {
	stage: TileErrorStage,
	count: AtomicU64,
	/// First `(coord, error message)` we saw for this stage. Used by both
	/// the live one-shot warning and the end-of-run summary.
	first: Mutex<Option<(TileCoord, String)>>,
}

impl StageStats {
	fn new(stage: TileErrorStage) -> Self {
		Self {
			stage,
			count: AtomicU64::new(0),
			first: Mutex::new(None),
		}
	}
}

impl TileErrorMonitor {
	#[must_use]
	pub fn new(label: &'static str) -> Self {
		Self {
			inner: Arc::new(MonitorInner {
				label,
				stages: [
					StageStats::new(TileErrorStage::Render),
					StageStats::new(TileErrorStage::Wrap),
					StageStats::new(TileErrorStage::Compress),
					StageStats::new(TileErrorStage::Serialize),
					StageStats::new(TileErrorStage::OverHardCap),
				],
			}),
		}
	}

	/// Record a per-tile error. The first error per stage gets a one-shot
	/// `log::warn!`; subsequent errors are only `log::debug!`. The total
	/// stays in the summary regardless.
	pub fn record(&self, coord: TileCoord, stage: TileErrorStage, error: &Error) {
		let slot = &self.inner.stages[stage as usize];
		let n = slot.count.fetch_add(1, Ordering::Relaxed);
		let msg = format!("{error:#}");
		if n == 0 {
			let mut first = slot.first.lock().expect("poisoned tile-error monitor mutex");
			*first = Some((coord, msg.clone()));
			drop(first);
			log::warn!(
				"{}: tile {}/{}/{} failed in `{}` stage: {msg}. Subsequent errors of this kind will only be logged at debug level; a summary will print at end of run.",
				self.inner.label,
				coord.level,
				coord.x,
				coord.y,
				stage.as_str(),
			);
		} else {
			log::debug!(
				"{}: tile {}/{}/{} failed in `{}` stage: {msg}",
				self.inner.label,
				coord.level,
				coord.x,
				coord.y,
				stage.as_str(),
			);
		}
	}
}

impl Drop for MonitorInner {
	fn drop(&mut self) {
		let total: u64 = self.stages.iter_mut().map(|s| *s.count.get_mut()).sum();
		if total == 0 {
			return; // monitor was created but no error ever flowed through; stay quiet.
		}
		let parts: Vec<String> = self
			.stages
			.iter_mut()
			.filter_map(|s| {
				let n = *s.count.get_mut();
				if n == 0 {
					return None;
				}
				let first = s.first.get_mut().expect("poisoned tile-error monitor mutex");
				let sample = first
					.as_ref()
					.map(|(c, m)| format!(" (first at {}/{}/{}: {m})", c.level, c.x, c.y))
					.unwrap_or_default();
				Some(format!("{}={n}{sample}", s.stage.as_str()))
			})
			.collect();
		log::warn!(
			"{}: {total} tile error(s); breakdown: [{}]",
			self.label,
			parts.join(", ")
		);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::anyhow;

	#[test]
	fn first_error_per_stage_increments_count() {
		let monitor = TileErrorMonitor::new("test");
		monitor.record(
			TileCoord::new(5, 1, 2).unwrap(),
			TileErrorStage::Render,
			&anyhow!("boom"),
		);
		monitor.record(
			TileCoord::new(5, 1, 3).unwrap(),
			TileErrorStage::Render,
			&anyhow!("boom again"),
		);
		assert_eq!(
			monitor.inner.stages[TileErrorStage::Render as usize]
				.count
				.load(Ordering::Relaxed),
			2
		);
	}

	#[test]
	fn other_stages_unaffected() {
		let monitor = TileErrorMonitor::new("test");
		monitor.record(
			TileCoord::new(0, 0, 0).unwrap(),
			TileErrorStage::Compress,
			&anyhow!("gzip oops"),
		);
		assert_eq!(
			monitor.inner.stages[TileErrorStage::Compress as usize]
				.count
				.load(Ordering::Relaxed),
			1
		);
		assert_eq!(
			monitor.inner.stages[TileErrorStage::Render as usize]
				.count
				.load(Ordering::Relaxed),
			0
		);
	}

	#[test]
	fn first_sample_is_recorded() {
		let monitor = TileErrorMonitor::new("test");
		monitor.record(
			TileCoord::new(7, 3, 4).unwrap(),
			TileErrorStage::Serialize,
			&anyhow!("bad blob"),
		);
		let first = monitor.inner.stages[TileErrorStage::Serialize as usize]
			.first
			.lock()
			.unwrap();
		let (c, msg) = first.as_ref().unwrap();
		assert_eq!((c.level, c.x, c.y), (7, 3, 4));
		assert_eq!(msg, "bad blob");
	}
}
