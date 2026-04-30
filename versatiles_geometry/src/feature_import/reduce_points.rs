//! Per-zoom point reduction strategies.
//!
//! Two strategies are implemented in v1:
//!
//! - `drop_rate`: keep `value^(max_zoom - z)` of the input points. The choice
//!   to keep or drop each point is deterministic via a stable per-feature
//!   hash, so the same input produces the same output and the kept set at
//!   `z = k` is a superset of the kept set at `z = k - 1`.
//! - `min_distance`: enforce a minimum mercator-meter distance between kept
//!   points. Uniform-grid bucketing with cell size = `threshold` checks the
//!   nine surrounding cells for any kept point inside the threshold.
//!   First-seen (input order) wins.
//!
//! Non-point geometries pass through both strategies unchanged.

use anyhow::{Result, bail};
use geo_types::{Coord, Geometry};
use std::collections::HashMap;

/// User-selected point-reduction strategy. Threshold-style values are stored
/// in [`crate::feature_import::FeatureImportConfig::min_distance_px`] and
/// [`crate::feature_import::FeatureImportConfig::drop_rate_keep_ratio`] —
/// only the field matching the active strategy is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PointReductionStrategy {
	None,
	DropRate,
	#[default]
	MinDistance,
}

impl PointReductionStrategy {
	/// Parse the string form used by the VPL `point_reduction=` argument.
	pub fn parse(name: &str) -> Result<Self> {
		match name {
			"none" => Ok(Self::None),
			"drop_rate" => Ok(Self::DropRate),
			"min_distance" => Ok(Self::MinDistance),
			other => bail!("unknown point_reduction strategy '{other}'; expected one of: none / drop_rate / min_distance"),
		}
	}
}

/// `drop_rate` filter. Keeps point features whose stable hash falls under
/// `keep_ratio`. Non-point features pass through.
#[must_use]
pub fn apply_drop_rate(features: Vec<(usize, Geometry<f64>)>, keep_ratio: f64) -> Vec<(usize, Geometry<f64>)> {
	if keep_ratio >= 1.0 {
		return features;
	}
	if keep_ratio <= 0.0 {
		// Drop all points; non-points still pass.
		return features.into_iter().filter(|(_, g)| !is_point(g)).collect();
	}
	features
		.into_iter()
		.filter(|(idx, g)| !is_point(g) || keep_for_index(*idx, keep_ratio))
		.collect()
}

/// `min_distance` filter. Drops point features that fall within `threshold`
/// (mercator-meters) of an already-kept point. Iteration order = input
/// order, so first-seen wins. Non-point features pass through.
#[must_use]
pub fn apply_min_distance(features: Vec<(usize, Geometry<f64>)>, threshold: f64) -> Vec<(usize, Geometry<f64>)> {
	if threshold <= 0.0 {
		return features;
	}
	let cell_size = threshold;
	let threshold_sq = threshold * threshold;
	let mut grid: HashMap<(i64, i64), Vec<Coord<f64>>> = HashMap::new();
	let mut kept = Vec::with_capacity(features.len());

	for (idx, geometry) in features {
		let Geometry::Point(point) = &geometry else {
			kept.push((idx, geometry));
			continue;
		};
		let coord = point.0;
		// `(x / cell_size).floor() as i64` saturates silently on NaN/inf;
		// drop non-finite points rather than indexing them into the grid.
		if !coord.x.is_finite() || !coord.y.is_finite() {
			continue;
		}
		#[allow(clippy::cast_possible_truncation)]
		let cx = (coord.x / cell_size).floor() as i64;
		#[allow(clippy::cast_possible_truncation)]
		let cy = (coord.y / cell_size).floor() as i64;
		let mut too_close = false;
		'outer: for dx in -1..=1_i64 {
			for dy in -1..=1_i64 {
				if let Some(cell_pts) = grid.get(&(cx + dx, cy + dy)) {
					for p in cell_pts {
						let dsq = (p.x - coord.x).powi(2) + (p.y - coord.y).powi(2);
						if dsq < threshold_sq {
							too_close = true;
							break 'outer;
						}
					}
				}
			}
		}
		if !too_close {
			grid.entry((cx, cy)).or_default().push(coord);
			kept.push((idx, geometry));
		}
	}
	kept
}

fn is_point(g: &Geometry<f64>) -> bool {
	matches!(g, Geometry::Point(_))
}

/// Stable per-index test: maps the original feature index to a uniform
/// `[0, 1)` value via splitmix64, returns `value < keep_ratio`.
fn keep_for_index(index: usize, keep_ratio: f64) -> bool {
	let h = splitmix64(index as u64);
	// Take the high 53 bits, divide by 2^53 — same approach as `f64::random`.
	#[allow(clippy::cast_precision_loss)]
	let u = (h >> 11) as f64 / (1u64 << 53) as f64;
	u < keep_ratio
}

/// Splitmix64 — a fast, deterministic, non-cryptographic mixer
/// (Steele, Lea, Flood 2014, "Fast Splittable Pseudorandom Number Generators").
fn splitmix64(mut x: u64) -> u64 {
	x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
	x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
	x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
	x ^ (x >> 31)
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)] // test indices are small `usize`s
mod tests {
	use super::*;
	use geo_types::{LineString, Point};

	fn point_geom(lon: f64, lat: f64) -> Geometry<f64> {
		Geometry::Point(Point::new(lon, lat))
	}

	#[test]
	fn parse_strategy_names() -> Result<()> {
		assert_eq!(PointReductionStrategy::parse("none")?, PointReductionStrategy::None);
		assert_eq!(
			PointReductionStrategy::parse("drop_rate")?,
			PointReductionStrategy::DropRate
		);
		assert_eq!(
			PointReductionStrategy::parse("min_distance")?,
			PointReductionStrategy::MinDistance
		);
		assert!(PointReductionStrategy::parse("unknown").is_err());
		Ok(())
	}

	#[test]
	fn drop_rate_keeps_all_at_ratio_1() {
		let features: Vec<(usize, Geometry<f64>)> = (0..100).map(|i| (i, point_geom(i as f64, 0.0))).collect();
		let kept = apply_drop_rate(features, 1.0);
		assert_eq!(kept.len(), 100);
	}

	#[test]
	fn drop_rate_drops_all_at_ratio_0() {
		let features: Vec<(usize, Geometry<f64>)> = (0..100).map(|i| (i, point_geom(i as f64, 0.0))).collect();
		let kept = apply_drop_rate(features, 0.0);
		assert_eq!(kept.len(), 0);
	}

	#[test]
	fn drop_rate_approximate_keep_ratio() {
		// 10000 points sampled at 0.5 should keep roughly 5000 (allow 5% tolerance).
		let features: Vec<(usize, Geometry<f64>)> = (0..10_000).map(|i| (i, point_geom(i as f64, 0.0))).collect();
		let kept = apply_drop_rate(features, 0.5);
		let ratio = kept.len() as f64 / 10_000.0;
		assert!((ratio - 0.5).abs() < 0.05, "expected ~0.5, got {ratio}");
	}

	#[test]
	fn drop_rate_is_deterministic() {
		let make = || -> Vec<(usize, Geometry<f64>)> { (0..1000).map(|i| (i, point_geom(i as f64, 0.0))).collect() };
		let kept1 = apply_drop_rate(make(), 0.3);
		let kept2 = apply_drop_rate(make(), 0.3);
		let indices1: Vec<usize> = kept1.iter().map(|(i, _)| *i).collect();
		let indices2: Vec<usize> = kept2.iter().map(|(i, _)| *i).collect();
		assert_eq!(indices1, indices2);
	}

	#[test]
	fn drop_rate_passes_non_points_unchanged() {
		let line: Geometry<f64> = Geometry::LineString(LineString::from(vec![[0.0, 0.0], [1.0, 1.0]]));
		let features: Vec<(usize, Geometry<f64>)> = vec![(0, line.clone())];
		let kept = apply_drop_rate(features, 0.0);
		assert_eq!(kept.len(), 1);
	}

	#[test]
	fn min_distance_drops_points_inside_threshold() {
		// Three points at 0, 5, 10 along the x-axis with threshold 7:
		// keep 0; drop 5 (within 7 of 0); keep 10 (within 5 of 5 — but 5 was
		// already dropped, so we check against KEPT points only → kept).
		let features = vec![
			(0, point_geom(0.0, 0.0)),
			(1, point_geom(5.0, 0.0)),
			(2, point_geom(10.0, 0.0)),
		];
		let kept = apply_min_distance(features, 7.0);
		assert_eq!(kept.len(), 2);
		assert_eq!(kept[0].0, 0);
		assert_eq!(kept[1].0, 2);
	}

	#[test]
	fn min_distance_first_seen_wins() {
		let features = vec![
			(0, point_geom(0.0, 0.0)),
			(1, point_geom(0.5, 0.0)), // < 1.0 from feature 0
			(2, point_geom(0.7, 0.0)), // < 1.0 from feature 0
		];
		let kept = apply_min_distance(features, 1.0);
		assert_eq!(kept.len(), 1);
		assert_eq!(kept[0].0, 0); // first-seen wins
	}

	#[test]
	fn min_distance_threshold_zero_keeps_everything() {
		let features = vec![
			(0, point_geom(0.0, 0.0)),
			(1, point_geom(0.0, 0.0)), // duplicate point
		];
		let kept = apply_min_distance(features, 0.0);
		assert_eq!(kept.len(), 2);
	}

	#[test]
	fn min_distance_passes_non_points_unchanged() {
		let line: Geometry<f64> = Geometry::LineString(LineString::from(vec![[0.0, 0.0], [1.0, 1.0]]));
		let features: Vec<(usize, Geometry<f64>)> = vec![(0, line.clone())];
		let kept = apply_min_distance(features, 100.0);
		assert_eq!(kept.len(), 1);
	}

	#[test]
	fn min_distance_grid_neighbor_check() {
		// Two points just inside the threshold but in different grid cells.
		// Cells are anchored at multiples of `threshold`, so these two points
		// straddle a cell boundary — the 9-cell scan must catch the conflict.
		let features = vec![
			(0, point_geom(9.99, 0.0)),  // cell (0, 0) when cell_size=10
			(1, point_geom(10.01, 0.0)), // cell (1, 0); distance to (9.99, 0) = 0.02
		];
		let kept = apply_min_distance(features, 10.0);
		assert_eq!(kept.len(), 1);
	}
}
