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
//!   aborting the conversion with a clear message including `(z, x, y)` and
//!   a breakdown of what's filling the tile (feature count, geometry vs.
//!   property bytes).
//! - **Soft cap** ([`SOFT_CAP_BYTES`], 200 KB) → track silently; emit a
//!   single one-shot warning the first time it's hit, with the same
//!   breakdown, so the user gets live feedback. Per-tile warnings would
//!   drown out the log on a multi-million-tile pyramid.
//! - **End-of-run summary** → on `Drop`, log total tile count, byte
//!   averages, and the top largest tiles (each with its breakdown) so
//!   the user can see exactly where the problem is.

use anyhow::{Result, bail};
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
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
		#[allow(clippy::cast_possible_truncation)]
		let mut feature_count = 0usize;
		let mut geometry_bytes = 0usize;
		let mut property_bytes = 0usize;
		for layer in &vt.layers {
			feature_count += layer.features.len();
			for f in &layer.features {
				#[allow(clippy::cast_possible_truncation)]
				let g = f.geom_data.len() as usize;
				geometry_bytes += g;
			}
			for k in layer.property_manager.iter_key() {
				property_bytes += k.len();
			}
			for v in layer.property_manager.iter_val() {
				if let Ok(blob) = GeoValuePBF::to_blob(v) {
					#[allow(clippy::cast_possible_truncation)]
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
	/// (compressed bytes, coord, breakdown) for the largest oversized tiles,
	/// reported at end-of-run.
	top: Mutex<Vec<(usize, TileCoord, TileBreakdown)>>,
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
	///
	/// `breakdown` is included in every warning / error message so the user
	/// can see whether geometry detail or property bloat is the dominant
	/// cost. Compute it via [`TileBreakdown::from_vector_tile`] before the
	/// underlying `VectorTile` is consumed by encoding.
	pub fn check(&self, coord: TileCoord, blob: &Blob, breakdown: TileBreakdown) -> Result<()> {
		#[allow(clippy::cast_possible_truncation)]
		let size = blob.len() as usize;
		if size > HARD_CAP_BYTES {
			bail!(
				"{}: tile {}/{}/{} is {} KB, exceeding the {} KB hard cap. \
				Breakdown: {}. \
				Likely causes: missing property pruning on a feature-heavy input, \
				`point_reduction='none'` on a dense point feed, or `max_zoom` set too low \
				so all features pile into a few tiles.",
				self.inner.label,
				coord.level,
				coord.x,
				coord.y,
				size / 1024,
				HARD_CAP_BYTES / 1024,
				breakdown.fmt_inline(),
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
					"{}: tile {}/{}/{} is {} KB (> {} KB soft cap). Breakdown: {}. \
					Further oversized tiles will be summarized at end of run.",
					inner.label,
					coord.level,
					coord.x,
					coord.y,
					size / 1024,
					SOFT_CAP_BYTES / 1024,
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
					.map(|(size, c, b)| {
						format!(
							"{}/{}/{} = {} KB ({})",
							c.level,
							c.x,
							c.y,
							size / 1024,
							b.fmt_inline(),
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
	use super::*;
	use versatiles_geometry::{
		geo::{GeoFeature, GeoProperties, GeoValue},
		vector_tile::{VectorTile, VectorTileLayer},
	};

	/// Synthesise a tiny breakdown so size-cap tests don't have to build a
	/// real `VectorTile`. Real-world numbers come through `from_vector_tile`.
	const TINY: TileBreakdown = TileBreakdown {
		feature_count: 0,
		geometry_bytes: 0,
		property_bytes: 0,
	};

	#[test]
	fn under_soft_cap_passes_silently() {
		let monitor = TileSizeMonitor::new("test");
		let blob = Blob::from(vec![0u8; 1024]);
		monitor.check(TileCoord::new(0, 0, 0).unwrap(), &blob, TINY).unwrap();
	}

	#[test]
	fn over_soft_cap_passes_but_records() {
		let monitor = TileSizeMonitor::new("test");
		let blob = Blob::from(vec![0u8; SOFT_CAP_BYTES + 1024]);
		monitor.check(TileCoord::new(5, 1, 2).unwrap(), &blob, TINY).unwrap();
		// One tile, one oversized.
		assert_eq!(monitor.inner.tile_count.load(Ordering::Relaxed), 1);
		assert_eq!(monitor.inner.oversized_count.load(Ordering::Relaxed), 1);
	}

	#[test]
	fn over_hard_cap_errors() {
		let monitor = TileSizeMonitor::new("test");
		let blob = Blob::from(vec![0u8; HARD_CAP_BYTES + 1]);
		let err = monitor
			.check(TileCoord::new(7, 3, 4).unwrap(), &blob, TINY)
			.unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("hard cap"), "{msg}");
		assert!(msg.contains("7/3/4"), "{msg}");
	}

	#[test]
	fn over_cap_message_includes_breakdown() {
		// The whole point of carrying TileBreakdown into `check`: the user
		// debugging an oversized tile sees feature count + geometry/property
		// bytes in the error message itself.
		let monitor = TileSizeMonitor::new("test");
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
		let monitor = TileSizeMonitor::new("test");
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
