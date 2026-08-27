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
//! - **Hard cap** (default [`HARD_CAP_BYTES`], 1 MiB; configurable per
//!   operation via `max_tile_bytes`, `none` disables) → return an error from
//!   `check`, aborting the conversion with a clear message including
//!   `(z, x, y)` and a breakdown of what's filling the tile (feature count,
//!   geometry vs. property bytes).
//! - **Soft cap** (default [`SOFT_CAP_BYTES`], 200 KB) → track silently; emit
//!   a single one-shot warning the first time it's hit, with the same
//!   breakdown, so the user gets live feedback. Per-tile warnings would
//!   drown out the log on a multi-million-tile pyramid. The soft cap scales
//!   with the configured hard cap (see [`soft_cap_for`]), so raising the hard
//!   cap doesn't leave the user warned about every tile they just opted in to.
//! - **End-of-run summary** → on `Drop`, log total tile count, byte
//!   averages, and the top largest tiles (each with its breakdown) so
//!   the user can see exactly where the problem is.

use std::sync::{
	Arc, Mutex,
	atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

use anyhow::{Result, anyhow, bail};
use versatiles_core::{Blob, TileCoord};
use versatiles_geometry::vector_tile::{GeoValuePBF, VectorTile};

/// What's filling a vector tile, broken down into actionable categories so
/// users debugging an oversized tile know where to cut.
///
/// The numbers measure **uncompressed** content, since that's what the user
/// can actually shrink (compression ratio is downstream). The compressed
/// blob size is reported separately by the surrounding warning.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TileBreakdown {
	/// Total feature count across all layers.
	pub feature_count: usize,
	/// Sum of `feature.geom_data.len()` across all layers.
	pub geometry_bytes: usize,
	/// Encoded property-table cost: key strings + values encoded as MVT
	/// `tile.layer.value` messages. Excludes per-feature `tag_ids` overhead
	/// (small relative to the table itself).
	pub property_bytes: usize,
}

impl TileBreakdown {
	/// Walk the tile's layers and sum the three categories.
	#[must_use]
	pub fn from_vector_tile(vt: &VectorTile) -> Self {
		// Blob lengths are u64; on 32-bit targets they could in principle
		// truncate, but a single MVT tile fits comfortably in a usize. Same
		// stance as the existing `blob.len() as usize` in `check`.
		let mut feature_count = 0usize;
		let mut geometry_bytes = 0usize;
		let mut property_bytes = 0usize;
		for layer in &vt.layers {
			feature_count += layer.features.len();
			for f in &layer.features {
				#[expect(
					clippy::cast_possible_truncation,
					reason = "a single MVT tile fits comfortably in a usize"
				)]
				let g = f.geom_data.len() as usize;
				geometry_bytes += g;
			}
			for k in layer.property_manager.iter_key() {
				property_bytes += k.len();
			}
			for v in layer.property_manager.iter_val() {
				if let Ok(blob) = GeoValuePBF::to_blob(v) {
					#[expect(
						clippy::cast_possible_truncation,
						reason = "a single MVT tile fits comfortably in a usize"
					)]
					let p = blob.len() as usize;
					property_bytes += p;
				}
			}
		}
		Self {
			feature_count,
			geometry_bytes,
			property_bytes,
		}
	}

	/// Render as a one-line breakdown for log lines.
	fn fmt_inline(&self) -> String {
		format!(
			"{} features; geometry {} KB, properties {} KB",
			self.feature_count,
			self.geometry_bytes / 1024,
			self.property_bytes / 1024,
		)
	}
}

/// Soft cap used when the hard cap is left at its default (or disabled).
/// Tiles below this size never trigger a warning.
pub const SOFT_CAP_BYTES: usize = 200 * 1024;

/// Default value for the hard cap when the operation doesn't configure one.
/// ~1 MiB is well past the practical limit of Mapbox / MapLibre style clients
/// and well into "this feed is misconfigured" territory.
pub const HARD_CAP_BYTES: usize = 1024 * 1024;

/// How many of the largest tiles to remember and report at end-of-run.
const TOP_N: usize = 5;

/// Format a byte count for log lines. Sizes are quoted in whole KB, except
/// below 1 KB where that would render as a bare "0 KB" — reachable now that
/// the cap is user-settable (`max_tile_bytes=500` says "500 bytes", not
/// "0 KB, exceeding the 0 KB hard cap").
fn fmt_bytes(bytes: usize) -> String {
	if bytes < 1024 {
		format!("{bytes} bytes")
	} else {
		format!("{} KB", bytes / 1024)
	}
}

/// Value of the VPL `max_tile_bytes` argument on the operations that
/// synthesise vector tiles.
///
/// Spelled as either a positive byte count (`max_tile_bytes=2097152`) or the
/// literal `none` (`max_tile_bytes=none`) to switch the hard cap off. `0` is
/// rejected rather than silently meaning "disabled": a zero-byte cap would
/// reject every tile, so taking it as its literal value is the more useful
/// reading, and `none` says "no cap" unambiguously.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaxTileBytes {
	/// `max_tile_bytes=none` — no hard cap; tiles are emitted at any size.
	Disabled,
	/// `max_tile_bytes=n` — cap encoded tiles at `n` bytes.
	Limit(u32),
}

impl MaxTileBytes {
	/// Parse the string form used by the VPL `max_tile_bytes=` argument.
	pub fn parse(value: &str) -> Result<Self> {
		let value = value.trim();
		if value.eq_ignore_ascii_case("none") {
			return Ok(Self::Disabled);
		}
		let bytes: u32 = value.parse().map_err(|_| {
			anyhow!("invalid max_tile_bytes '{value}'; expected a byte count (e.g. 2097152) or 'none' to disable the cap")
		})?;
		if bytes == 0 {
			bail!("max_tile_bytes=0 would reject every tile; use max_tile_bytes=none to disable the cap");
		}
		Ok(Self::Limit(bytes))
	}

	/// The hard cap this value implies, in bytes; `None` when disabled.
	#[must_use]
	pub fn hard_cap_bytes(self) -> Option<usize> {
		match self {
			Self::Disabled => None,
			Self::Limit(n) => Some(n as usize),
		}
	}
}

impl TryFrom<&str> for MaxTileBytes {
	type Error = anyhow::Error;
	fn try_from(value: &str) -> Result<Self> {
		Self::parse(value)
	}
}

/// Map the optional VPL `max_tile_bytes` argument to the monitor's hard cap:
/// absent → the default [`HARD_CAP_BYTES`] (1 MiB); `none` → no cap at all;
/// `n` → cap encoded tiles at `n` bytes.
#[must_use]
pub fn resolve_hard_cap(max_tile_bytes: Option<MaxTileBytes>) -> Option<usize> {
	max_tile_bytes.map_or(Some(HARD_CAP_BYTES), MaxTileBytes::hard_cap_bytes)
}

/// Soft-cap threshold for a given hard cap.
///
/// The soft cap is advisory ("this tile is getting big"), so it only means
/// something relative to the size the user actually considers acceptable.
/// Scaling it by the hard cap keeps the default 200 KB : 1 MiB ratio: a user
/// who raises the cap to 2 MiB starts hearing about tiles at 400 KB rather
/// than being warned about every tile they deliberately allowed, and a user
/// who lowers it to 100 KB still gets a warning before the hard failure.
///
/// A disabled hard cap keeps the plain [`SOFT_CAP_BYTES`] default — there's
/// no ceiling to scale against, and the advisory warning is the only size
/// feedback left.
#[must_use]
fn soft_cap_for(hard_cap_bytes: Option<usize>) -> usize {
	let Some(hard) = hard_cap_bytes else {
		return SOFT_CAP_BYTES;
	};
	let soft = u64::try_from(SOFT_CAP_BYTES)
		.unwrap_or(u64::MAX)
		.saturating_mul(u64::try_from(hard).unwrap_or(u64::MAX))
		/ u64::try_from(HARD_CAP_BYTES).unwrap_or(u64::MAX);
	// `.max(1)` keeps a pathologically small hard cap (say 1 byte, as the
	// tests use) from warning about every tile it still accepts.
	usize::try_from(soft).unwrap_or(usize::MAX).max(1)
}

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
	/// Encoded-blob size above which `check` returns an error. `None` disables
	/// the hard cap entirely (only the soft-cap warning remains).
	hard_cap_bytes: Option<usize>,
	/// Encoded-blob size above which a tile counts as oversized and gets
	/// warned about. Derived from `hard_cap_bytes` by [`soft_cap_for`].
	soft_cap_bytes: usize,
	tile_count: AtomicU64,
	oversized_count: AtomicU64,
	total_bytes: AtomicU64,
	max_bytes: AtomicUsize,
	first_oversized_warned: AtomicBool,
	/// (compressed bytes, coord, breakdown) for the largest oversized tiles,
	/// reported at end-of-run.
	top: Mutex<Vec<(usize, TileCoord, TileBreakdown)>>,
}

impl TileSizeMonitor {
	#[must_use]
	pub fn new(label: &'static str, hard_cap_bytes: Option<usize>) -> Self {
		Self {
			inner: Arc::new(MonitorInner {
				label,
				hard_cap_bytes,
				soft_cap_bytes: soft_cap_for(hard_cap_bytes),
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
	/// update accounting. Errors when above the configured hard cap
	/// ([`HARD_CAP_BYTES`] by default; `None` disables the hard cap).
	///
	/// `breakdown` is included in every warning / error message so the user
	/// can see whether geometry detail or property bloat is the dominant
	/// cost. Compute it via [`TileBreakdown::from_vector_tile`] before the
	/// underlying `VectorTile` is consumed by encoding.
	pub fn check(&self, coord: TileCoord, blob: &Blob, breakdown: TileBreakdown) -> Result<()> {
		#[expect(
			clippy::cast_possible_truncation,
			reason = "a single MVT tile fits comfortably in a usize"
		)]
		let size = blob.len() as usize;
		let inner = &self.inner;
		if let Some(hard_cap) = inner.hard_cap_bytes
			&& size > hard_cap
		{
			bail!(
				"{}: tile {}/{}/{} is {}, exceeding the {} hard cap. \
				Breakdown: {}. \
				Likely causes: missing property pruning on a feature-heavy input, \
				`point_reduction='none'` on a dense point feed, or `max_zoom` set too low \
				so all features pile into a few tiles. Raise the `max_tile_bytes` argument \
				to accept larger tiles (`max_tile_bytes=none` disables the cap).",
				inner.label,
				coord.level,
				coord.x,
				coord.y,
				fmt_bytes(size),
				fmt_bytes(hard_cap),
				breakdown.fmt_inline(),
			);
		}

		inner.tile_count.fetch_add(1, Ordering::Relaxed);
		inner.total_bytes.fetch_add(size as u64, Ordering::Relaxed);
		inner.max_bytes.fetch_max(size, Ordering::Relaxed);

		if size > inner.soft_cap_bytes {
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
					"{}: tile {}/{}/{} is {} (> {} soft cap). Breakdown: {}. \
					Further oversized tiles will be summarized at end of run.",
					inner.label,
					coord.level,
					coord.x,
					coord.y,
					fmt_bytes(size),
					fmt_bytes(inner.soft_cap_bytes),
					breakdown.fmt_inline(),
				);
			}
			let mut top = inner.top.lock().expect("poisoned tile-size monitor mutex");
			top.push((size, coord, breakdown));
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
			"{}: emitted {count} tile(s); avg {} bytes, max {} bytes; {oversized} over {}",
			self.label,
			avg,
			max,
			fmt_bytes(self.soft_cap_bytes),
		);
		if oversized > 0 {
			let top = self.top.get_mut().expect("poisoned tile-size monitor mutex");
			if !top.is_empty() {
				let lines: Vec<String> = top
					.iter()
					.map(|(size, c, b)| {
						format!(
							"{}/{}/{} = {} ({})",
							c.level,
							c.x,
							c.y,
							fmt_bytes(*size),
							b.fmt_inline()
						)
					})
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
	use versatiles_geometry::{
		geo::{GeoFeature, GeoProperties, GeoValue},
		vector_tile::{VectorTile, VectorTileLayer},
	};

	use super::*;

	/// Synthesise a tiny breakdown so size-cap tests don't have to build a
	/// real `VectorTile`. Real-world numbers come through `from_vector_tile`.
	const TINY: TileBreakdown = TileBreakdown {
		feature_count: 0,
		geometry_bytes: 0,
		property_bytes: 0,
	};

	#[test]
	fn under_soft_cap_passes_silently() {
		let monitor = TileSizeMonitor::new("test", Some(HARD_CAP_BYTES));
		let blob = Blob::from(vec![0u8; 1024]);
		monitor.check(TileCoord::new(0, 0, 0).unwrap(), &blob, TINY).unwrap();
	}

	#[test]
	fn over_soft_cap_passes_but_records() {
		let monitor = TileSizeMonitor::new("test", Some(HARD_CAP_BYTES));
		let blob = Blob::from(vec![0u8; SOFT_CAP_BYTES + 1024]);
		monitor.check(TileCoord::new(5, 1, 2).unwrap(), &blob, TINY).unwrap();
		// One tile, one oversized.
		assert_eq!(monitor.inner.tile_count.load(Ordering::Relaxed), 1);
		assert_eq!(monitor.inner.oversized_count.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn over_hard_cap_errors() {
		let monitor = TileSizeMonitor::new("test", Some(HARD_CAP_BYTES));
		let blob = Blob::from(vec![0u8; HARD_CAP_BYTES + 1]);
		let err = monitor
			.check(TileCoord::new(7, 3, 4).unwrap(), &blob, TINY)
			.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("hard cap"), "{msg}");
		assert!(msg.contains("7/3/4"), "{msg}");
	}

	#[test]
	fn disabled_hard_cap_passes_oversized_tile() {
		// `max_tile_bytes=none` → hard cap disabled: an over-1-MiB tile must
		// pass the hard-cap check (only soft-cap accounting applies).
		let monitor = TileSizeMonitor::new("test", None);
		let blob = Blob::from(vec![0u8; HARD_CAP_BYTES + 1024]);
		monitor.check(TileCoord::new(7, 3, 4).unwrap(), &blob, TINY).unwrap();
	}

	// ── MaxTileBytes parsing ─────────────────────────────────────────────

	#[test]
	fn max_tile_bytes_parses_none_and_byte_counts() {
		assert_eq!(MaxTileBytes::parse("none").unwrap(), MaxTileBytes::Disabled);
		assert_eq!(MaxTileBytes::parse("NONE").unwrap(), MaxTileBytes::Disabled);
		assert_eq!(MaxTileBytes::parse(" none ").unwrap(), MaxTileBytes::Disabled);
		assert_eq!(MaxTileBytes::parse("2097152").unwrap(), MaxTileBytes::Limit(2097152));
	}

	#[test]
	fn max_tile_bytes_rejects_zero_and_garbage() {
		// `0` is not a synonym for "disabled": taken literally it would reject
		// every tile, so it's an error that points at the real spelling.
		let err = format!("{:#}", MaxTileBytes::parse("0").unwrap_err());
		assert!(err.contains("max_tile_bytes=none"), "{err}");

		let err = format!("{:#}", MaxTileBytes::parse("2MiB").unwrap_err());
		assert!(err.contains("expected a byte count"), "{err}");
	}

	#[test]
	fn max_tile_bytes_maps_to_hard_cap() {
		assert_eq!(MaxTileBytes::Disabled.hard_cap_bytes(), None);
		assert_eq!(MaxTileBytes::Limit(4096).hard_cap_bytes(), Some(4096));
		// Absent argument keeps the built-in default.
		assert_eq!(resolve_hard_cap(None), Some(HARD_CAP_BYTES));
		assert_eq!(resolve_hard_cap(Some(MaxTileBytes::Disabled)), None);
		assert_eq!(resolve_hard_cap(Some(MaxTileBytes::Limit(4096))), Some(4096));
	}

	#[test]
	fn small_sizes_are_reported_in_bytes() {
		// A sub-KB cap must not render as "0 KB, exceeding the 0 KB hard cap".
		assert_eq!(fmt_bytes(500), "500 bytes");
		assert_eq!(fmt_bytes(1023), "1023 bytes");
		assert_eq!(fmt_bytes(1024), "1 KB");

		let monitor = TileSizeMonitor::new("test", Some(10));
		let blob = Blob::from(vec![0u8; 100]);
		let err = monitor
			.check(TileCoord::new(0, 0, 0).unwrap(), &blob, TINY)
			.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("is 100 bytes"), "{msg}");
		assert!(msg.contains("10 bytes hard cap"), "{msg}");
	}

	// ── soft cap scaling ─────────────────────────────────────────────────

	#[test]
	fn soft_cap_scales_with_hard_cap() {
		// Default hard cap reproduces the historical soft cap exactly.
		assert_eq!(soft_cap_for(Some(HARD_CAP_BYTES)), SOFT_CAP_BYTES);
		// Doubling the hard cap doubles the advisory threshold.
		assert_eq!(soft_cap_for(Some(2 * HARD_CAP_BYTES)), 2 * SOFT_CAP_BYTES);
		// Halving it halves the threshold, so a warning still precedes the error.
		assert_eq!(soft_cap_for(Some(HARD_CAP_BYTES / 2)), SOFT_CAP_BYTES / 2);
		// No ceiling to scale against → plain default.
		assert_eq!(soft_cap_for(None), SOFT_CAP_BYTES);
		// A pathologically small cap must not warn about tiles it accepts.
		assert_eq!(soft_cap_for(Some(1)), 1);
	}

	#[test]
	fn raised_hard_cap_does_not_warn_about_tiles_it_allows() {
		// The point of scaling: at a 4 MiB cap, a 300 KB tile is unremarkable
		// and must not be counted as oversized, even though it clears the
		// stock 200 KB soft cap.
		let monitor = TileSizeMonitor::new("test", Some(4 * HARD_CAP_BYTES));
		let blob = Blob::from(vec![0u8; 300 * 1024]);
		monitor.check(TileCoord::new(5, 1, 2).unwrap(), &blob, TINY).unwrap();
		assert_eq!(monitor.inner.oversized_count.load(Ordering::Relaxed), 0);

		// 900 KB is past the scaled 800 KB soft cap, so it is recorded.
		let blob = Blob::from(vec![0u8; 900 * 1024]);
		monitor.check(TileCoord::new(5, 1, 3).unwrap(), &blob, TINY).unwrap();
		assert_eq!(monitor.inner.oversized_count.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn lowered_hard_cap_still_warns_before_it_errors() {
		// At a 100 KB cap the stock 200 KB soft cap would never fire — the
		// hard cap would error first, so the user gets no advance warning.
		let monitor = TileSizeMonitor::new("test", Some(100 * 1024));
		let blob = Blob::from(vec![0u8; 50 * 1024]);
		monitor.check(TileCoord::new(5, 1, 2).unwrap(), &blob, TINY).unwrap();
		assert_eq!(monitor.inner.oversized_count.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn custom_hard_cap_errors_at_custom_threshold() {
		// A per-operation cap of 512 KB: 600 KB errors, 400 KB passes.
		let monitor = TileSizeMonitor::new("test", Some(512 * 1024));
		let over = Blob::from(vec![0u8; 600 * 1024]);
		monitor
			.check(TileCoord::new(3, 1, 1).unwrap(), &over, TINY)
			.unwrap_err();
		let under = Blob::from(vec![0u8; 400 * 1024]);
		monitor.check(TileCoord::new(3, 1, 1).unwrap(), &under, TINY).unwrap();
	}

	#[test]
	fn over_cap_message_includes_breakdown() {
		// The whole point of carrying TileBreakdown into `check`: the user
		// debugging an oversized tile sees feature count + geometry/property
		// bytes in the error message itself.
		let monitor = TileSizeMonitor::new("test", Some(HARD_CAP_BYTES));
		let blob = Blob::from(vec![0u8; HARD_CAP_BYTES + 1]);
		let breakdown = TileBreakdown {
			feature_count: 1234,
			geometry_bytes: 600 * 1024,
			property_bytes: 50 * 1024,
		};
		let err = monitor
			.check(TileCoord::new(0, 0, 0).unwrap(), &blob, breakdown)
			.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("1234 features"), "{msg}");
		assert!(msg.contains("geometry 600 KB"), "{msg}");
		assert!(msg.contains("properties 50 KB"), "{msg}");
	}

	#[test]
	fn top_n_keeps_largest() {
		let monitor = TileSizeMonitor::new("test", Some(HARD_CAP_BYTES));
		// Five tiles, ascending sizes — only the 5 largest stay (which is all).
		// Add a 6th and verify the smallest is dropped.
		let sizes_kb = [210, 220, 230, 240, 250, 260];
		for (i, kb) in sizes_kb.iter().enumerate() {
			let blob = Blob::from(vec![0u8; kb * 1024]);
			// z=4 has plenty of x slots; the exact coord doesn't matter for the test.
			let coord = TileCoord::new(4, u32::try_from(i).unwrap(), 0).unwrap();
			monitor.check(coord, &blob, TINY).unwrap();
		}
		let top = monitor.inner.top.lock().unwrap();
		assert_eq!(top.len(), TOP_N);
		// Largest first, smallest in the kept window is 220 KB.
		assert_eq!(top[0].0, 260 * 1024);
		assert_eq!(top.last().unwrap().0, 220 * 1024);
	}

	#[test]
	fn breakdown_from_vector_tile_counts_real_bytes() {
		// One layer, two point features, three properties → exercise every
		// branch of the breakdown sum: feature count, geometry bytes (the
		// raw geom_data per feature), key strings, and encoded values.
		use geo_types::{Geometry, Point};
		let f1 = GeoFeature {
			id: None,
			geometry: Geometry::Point(Point::new(0.0, 0.0)),
			properties: GeoProperties::from_iter(vec![
				("name".to_string(), GeoValue::from("Berlin")),
				("pop".to_string(), GeoValue::from(3_645_000_u64)),
			]),
		};
		let f2 = GeoFeature {
			id: None,
			geometry: Geometry::Point(Point::new(1.0, 1.0)),
			properties: GeoProperties::from_iter(vec![("name".to_string(), GeoValue::from("Munich"))]),
		};
		let layer = VectorTileLayer::from_features("places".to_string(), vec![f1, f2], 4096, 1).unwrap();
		let vt = VectorTile::new(vec![layer]);

		let b = TileBreakdown::from_vector_tile(&vt);
		assert_eq!(b.feature_count, 2);
		assert!(b.geometry_bytes > 0, "two points should produce some geom bytes");
		// "name" appears once (deduped), "pop" appears once → key bytes = 4 + 3 = 7,
		// plus three values: "Berlin" (string), "Munich" (string), 3645000 (uint).
		assert!(b.property_bytes > 7, "property bytes should include key + value table");
	}
}
