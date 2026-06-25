//! Shared tile-sampling utilities.
//!
//! Used by `probe -ddd` and `validate-schema` to let callers read a
//! representative subset of a large tile pyramid rather than every tile.
//! Tiles are grouped into contiguous square *windows* so remote sources
//! can coalesce multiple coordinate lookups into a single byte-range request.

use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use versatiles_core::TileBBox;

/// Side length (in tiles) of each sampling window. 64 keeps a window well
/// inside a single 256×256 versatiles block → one coalesced range read.
pub const WINDOW_SIZE: u32 = 64;

/// Validates `--sample PERCENT` and converts it to a `(0, 1]` fraction.
/// `None` (flag absent) means scan every tile.
pub fn parse_sample(percent: Option<f64>) -> Result<Option<f64>> {
	match percent {
		None => Ok(None),
		Some(p) if p.is_finite() && p > 0.0 && p <= 100.0 => Ok(Some(p / 100.0)),
		Some(p) => bail!("--sample must be in the range (0, 100], got {p}"),
	}
}

/// Splitmix64 finaliser — cheap, well-distributed avalanche mixing.
pub fn mix64(mut z: u64) -> u64 {
	z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
	z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
	z ^ (z >> 31)
}

/// Returns the number of windows per level that approximate `fraction` of the
/// deepest level (the level with the most tiles). Applying this count to every
/// level means shallower levels are sampled more fully. Always at least 1.
#[allow(
	clippy::cast_precision_loss,
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss
)]
pub fn windows_for_sample(fraction: f64, deepest_tiles: u64) -> u32 {
	let target = (fraction * deepest_tiles as f64).ceil() as u64;
	let per_window = u64::from(WINDOW_SIZE) * u64::from(WINDOW_SIZE);
	u32::try_from(target.div_ceil(per_window).max(1)).unwrap_or(u32::MAX)
}

/// Picks up to `k` deterministic square windows of side `s` inside `bbox`.
/// When `k` such windows would already cover the whole level, the level bbox
/// is returned unsplit — small low-zoom levels get scanned in full.
/// Placement is hash-based so the same (level, bbox, k) always yields the same
/// windows.
pub fn plan_windows(level: u8, bbox: &TileBBox, k: u32, s: u32) -> Result<Vec<TileBBox>> {
	let (cols, rows) = (bbox.width(), bbox.height());
	let (sw, sh) = (s.min(cols), s.min(rows));
	if u64::from(k) * u64::from(sw) * u64::from(sh) >= bbox.count_tiles() {
		return Ok(vec![*bbox]);
	}
	let (x_min, y_min) = (bbox.x_min()?, bbox.y_min()?);
	let (span_x, span_y) = (cols - sw + 1, rows - sh + 1);

	let mut corners: BTreeSet<(u32, u32)> = BTreeSet::new();
	let mut i = 0u32;
	while corners.len() < k as usize && i < k.saturating_mul(8).max(k) {
		let seed = (u64::from(level) << 40) ^ (u64::from(i) << 1);
		let x0 = x_min + u32::try_from(mix64(seed) % u64::from(span_x)).unwrap_or(0);
		let y0 = y_min + u32::try_from(mix64(seed ^ 1) % u64::from(span_y)).unwrap_or(0);
		corners.insert((x0, y0));
		i += 1;
	}

	corners
		.into_iter()
		.map(|(x0, y0)| TileBBox::from_min_and_max(level, x0, y0, x0 + sw - 1, y0 + sh - 1))
		.collect()
}

/// Builds the list of `TileBBox` windows to scan for a given tile pyramid.
///
/// - `sample = None` → one bbox per non-empty level (full scan).
/// - `sample = Some(fraction)` → up to `windows_for_sample(fraction, …)`
///   windows per level, with placement chosen to keep remote reads cheap.
pub fn build_scan_plan(bboxes: impl Iterator<Item = TileBBox>, sample: Option<f64>) -> Result<Vec<TileBBox>> {
	let bboxes: Vec<TileBBox> = bboxes.filter(|b| !b.is_empty()).collect();
	if sample.is_none() {
		return Ok(bboxes);
	}
	let fraction = sample.unwrap();
	let deepest = bboxes.iter().map(TileBBox::count_tiles).max().unwrap_or(0);
	let k = windows_for_sample(fraction, deepest);
	let mut plan = Vec::new();
	for bbox in &bboxes {
		plan.extend(
			plan_windows(bbox.level(), bbox, k, WINDOW_SIZE)
				.with_context(|| format!("building sample windows for level {}", bbox.level()))?,
		);
	}
	Ok(plan)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_sample_validates_range() {
		assert_eq!(parse_sample(None).unwrap(), None);
		assert_eq!(parse_sample(Some(100.0)).unwrap(), Some(1.0));
		assert_eq!(parse_sample(Some(10.0)).unwrap(), Some(0.1));
		assert!(parse_sample(Some(0.0)).is_err());
		assert!(parse_sample(Some(-5.0)).is_err());
		assert!(parse_sample(Some(150.0)).is_err());
		assert!(parse_sample(Some(f64::NAN)).is_err());
	}

	#[test]
	fn windows_for_sample_scales_and_floors() {
		assert_eq!(windows_for_sample(1.0, 16_384), 4);
		assert_eq!(windows_for_sample(0.1, 16_384), 1);
		assert_eq!(windows_for_sample(0.1, 4_000_000), 98);
		assert_eq!(windows_for_sample(0.001, 1), 1);
	}

	#[test]
	fn plan_windows_is_deterministic_and_bounded() {
		let bbox = TileBBox::from_min_and_max(14, 0, 0, 1023, 1023).unwrap();
		let a = plan_windows(14, &bbox, 4, WINDOW_SIZE).unwrap();
		let b = plan_windows(14, &bbox, 4, WINDOW_SIZE).unwrap();
		assert_eq!(a, b, "window placement must be deterministic");
		assert_eq!(a.len(), 4);
		for w in &a {
			assert_eq!(w.count_tiles(), u64::from(WINDOW_SIZE) * u64::from(WINDOW_SIZE));
		}
	}

	#[test]
	fn plan_windows_covers_small_level_whole() {
		let bbox = TileBBox::from_min_and_max(3, 0, 0, 7, 7).unwrap();
		let plan = plan_windows(3, &bbox, 4, WINDOW_SIZE).unwrap();
		assert_eq!(plan, vec![bbox]);
	}

	#[test]
	fn build_scan_plan_no_sample_returns_all_bboxes() {
		let bboxes = vec![
			TileBBox::from_min_and_max(0, 0, 0, 0, 0).unwrap(),
			TileBBox::from_min_and_max(1, 0, 0, 1, 1).unwrap(),
		];
		let plan = build_scan_plan(bboxes.into_iter(), None).unwrap();
		assert_eq!(plan.len(), 2);
	}

	#[test]
	fn build_scan_plan_filters_empty_bboxes() {
		let bboxes = vec![
			TileBBox::new_empty(0).unwrap(),
			TileBBox::from_min_and_max(1, 0, 0, 1, 1).unwrap(),
		];
		let plan = build_scan_plan(bboxes.into_iter(), None).unwrap();
		assert_eq!(plan.len(), 1);
	}
}
