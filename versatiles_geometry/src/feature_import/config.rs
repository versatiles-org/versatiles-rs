//! Configuration shapes for [`super::FeatureImport`].
//!
//! Two related types live here:
//!
//! - [`FeatureImportArgs`] — the user-input shape. Every field is optional;
//!   public callers (the `from_geo` / `from_csv` VPL operations) build one
//!   as a near 1:1 copy of their own arg structs and pass it to
//!   [`super::FeatureImport::from_features`].
//! - [`FeatureImportConfig`] — the resolved shape consumed by the cascade.
//!   Every field is concrete; defaults have been applied. Built from a
//!   [`FeatureImportArgs`] via `From`.
//!
//! Keeping the two distinct lets defaults live in exactly one place
//! ([`FeatureImportConfig::default`]) and keeps the cascade reading concrete
//! values without `unwrap_or` noise on every access.

use super::PointReductionStrategy;

/// User-input shape for [`super::FeatureImport`]: every knob optional.
///
/// `max_zoom` keeps its `Option<u8>` shape on both sides — `None` is a
/// meaningful signal there ("run the auto-heuristic"), not a missing value.
#[derive(Clone, Debug, Default)]
pub struct FeatureImportArgs {
	pub layer_name: Option<String>,
	pub min_zoom: Option<u8>,
	/// Highest zoom level emitted. `None` triggers the auto-heuristic
	/// (median feature size ≈ 4 tile-pixels, capped at 14).
	pub max_zoom: Option<u8>,
	pub polygon_simplify_px: Option<f32>,
	pub line_simplify_px: Option<f32>,
	pub polygon_min_area_px: Option<f32>,
	pub line_min_length_px: Option<f32>,
	pub point_reduction: Option<PointReductionStrategy>,
	pub point_reduction_value: Option<f32>,
}

/// Resolved configuration consumed by the cascade. Every field is concrete;
/// defaults have already been applied. Built from a [`FeatureImportArgs`]
/// (typical) or constructed directly (advanced/test code).
#[derive(Clone, Debug)]
pub struct FeatureImportConfig {
	pub layer_name: String,
	pub min_zoom: u8,
	/// Highest zoom level emitted. `None` triggers the auto-heuristic
	/// (median feature size ≈ 4 tile-pixels, capped at 14).
	pub max_zoom: Option<u8>,
	/// Douglas-Peucker tolerance for polygons, in tile-pixels at the current zoom.
	pub polygon_simplify_px: f32,
	/// Douglas-Peucker tolerance for lines, in tile-pixels at the current zoom.
	pub line_simplify_px: f32,
	/// Drop polygons whose area at the current zoom is below this many tile-pixels².
	/// `0.0` disables the filter.
	pub polygon_min_area_px: f32,
	/// Drop lines whose length at the current zoom is below this many tile-pixels.
	/// `0.0` disables the filter.
	pub line_min_length_px: f32,
	/// Point-reduction strategy applied per-zoom; cumulative across zooms (a
	/// point dropped at zoom z+1 cannot reappear at zoom z). See
	/// [`PointReductionStrategy`].
	pub point_reduction: PointReductionStrategy,
	/// Threshold for [`PointReductionStrategy::MinDistance`]: minimum distance
	/// between kept points, in tile-pixels *at the current zoom*. Equivalent
	/// to a coarser threshold (in meters) at lower zooms. Ignored unless
	/// `point_reduction` is `MinDistance`.
	pub min_distance_px: f32,
	/// Per-zoom keep-fraction for [`PointReductionStrategy::DropRate`]
	/// (in `[0, 1]`). Composes geometrically across zooms — at `max_zoom - k`,
	/// the cumulative keep-ratio is `value^k`. Ignored unless
	/// `point_reduction` is `DropRate`.
	pub drop_rate_keep_ratio: f32,
}

impl Default for FeatureImportConfig {
	fn default() -> Self {
		Self {
			layer_name: "features".to_string(),
			min_zoom: 0,
			max_zoom: None, // auto via `auto_max_zoom`
			polygon_simplify_px: 4.0,
			line_simplify_px: 4.0,
			polygon_min_area_px: 4.0,
			line_min_length_px: 4.0,
			// Point datasets at city/regional density tend to be unreadable
			// without thinning at low zooms; min-distance with a 16-pixel
			// threshold is the sensible default that "just works".
			point_reduction: PointReductionStrategy::MinDistance,
			min_distance_px: 16.0,
			// 0.5 ≈ "halve the survivors per zoom step out from max_zoom" —
			// a sane default thinning curve. Only used when `point_reduction`
			// is explicitly switched to `DropRate`.
			drop_rate_keep_ratio: 0.5,
		}
	}
}

impl From<FeatureImportArgs> for FeatureImportConfig {
	fn from(args: FeatureImportArgs) -> Self {
		let d = Self::default();
		// The user-facing API exposes a single `point_reduction_value` knob
		// whose meaning depends on the active strategy. Route it to the
		// strategy-appropriate field; the other field stays at default and
		// will be ignored by the cascade.
		let strategy = args.point_reduction.unwrap_or(d.point_reduction);
		let (min_distance_px, drop_rate_keep_ratio) = match (strategy, args.point_reduction_value) {
			(PointReductionStrategy::MinDistance, Some(v)) => (v, d.drop_rate_keep_ratio),
			(PointReductionStrategy::DropRate, Some(v)) => (d.min_distance_px, v),
			_ => (d.min_distance_px, d.drop_rate_keep_ratio),
		};
		Self {
			layer_name: args.layer_name.unwrap_or(d.layer_name),
			min_zoom: args.min_zoom.unwrap_or(d.min_zoom),
			// `Option::or` here, not `unwrap_or`: the "default" for max_zoom
			// is itself `None` (= run the auto-heuristic later).
			max_zoom: args.max_zoom.or(d.max_zoom),
			polygon_simplify_px: args.polygon_simplify_px.unwrap_or(d.polygon_simplify_px),
			line_simplify_px: args.line_simplify_px.unwrap_or(d.line_simplify_px),
			polygon_min_area_px: args.polygon_min_area_px.unwrap_or(d.polygon_min_area_px),
			line_min_length_px: args.line_min_length_px.unwrap_or(d.line_min_length_px),
			point_reduction: strategy,
			min_distance_px,
			drop_rate_keep_ratio,
		}
	}
}
