//! `versatiles dev check-shortbread` — validate a vector-tile container against
//! the [Shortbread](https://shortbread-tiles.org/) schema.
//!
//! Every tile is decoded and checked, per layer/feature/attribute, against an
//! embedded copy of the schema (versions 1.0 and 1.1). Findings are aggregated
//! into counted issue groups and printed as a summary, a full list, or JSON.

mod report;
mod schema;
mod validate;

use anyhow::{Context, Result, bail};
use report::{Registry, Severity};
use schema::{Schema, SchemaVersion};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use versatiles_container::TilesRuntime;
use versatiles_core::{TileBBox, TileType};

/// How to present the findings.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum OutputFormat {
	/// Counts grouped by severity and rule, with a few example tiles each.
	Summary,
	/// Every distinct finding on its own line.
	List,
	/// Machine-readable JSON array of findings.
	Json,
}

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
/// Check that the vector tiles in a container follow the Shortbread schema.
///
/// Reports unknown/missing layers and attributes, wrong value types or
/// geometries, and out-of-vocabulary values. Problems are graded as errors,
/// warnings, or hints; unknown values are tolerated by default (use `--strict`
/// to treat them as hard failures).
pub struct CheckShortbread {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// Schema version to validate against (`auto` picks the best match).
	#[arg(long, value_enum, default_value = "auto")]
	schema: SchemaVersion,

	/// Restrict scanning to a zoom level `N` or an inclusive range `A-B`.
	#[arg(long, value_name = "N|A-B")]
	zoom: Option<String>,

	/// Sample only part of the container instead of every tile — far fewer reads
	/// from remote sources. PERCENT (0–100) is the approximate share of the
	/// deepest zoom level to read; shallower levels are covered more fully. Tiles
	/// are read in contiguous square windows (deterministic placement) so each
	/// window maps to one coalesced range request.
	#[arg(long, value_name = "PERCENT", verbatim_doc_comment)]
	sample: Option<f64>,

	/// Output format.
	#[arg(long, value_enum, default_value = "summary")]
	format: OutputFormat,

	/// Hide findings below this severity.
	#[arg(long, value_enum, default_value = "hint")]
	min_severity: Severity,

	/// Treat unknown layers/attributes/values as one severity level higher.
	#[arg(long)]
	strict: bool,

	/// Exit with a non-zero status if any finding reaches this severity.
	#[arg(long, value_enum, default_value = "error")]
	fail_on: Severity,
}

pub async fn run(args: &CheckShortbread, runtime: &TilesRuntime) -> Result<()> {
	// Validate cheap arguments before touching the container.
	let sample = parse_sample(args.sample)?;

	let reader = runtime.reader_from_str(&args.input).await?;

	if reader.metadata().tile_format().to_type() != TileType::Vector {
		bail!(
			"input is not a vector tile source (format: {:?})",
			reader.metadata().tile_format()
		);
	}

	// Layer names declared in the TileJSON help auto-detect the schema version.
	let present_layers: Vec<String> = reader.tilejson().vector_layers.iter().map(|(k, _)| k.clone()).collect();
	let schema = Arc::new(Schema::resolve(args.schema, &present_layers)?);

	let pyramid = reader.tile_pyramid().await?;
	let pmin = pyramid.level_min().unwrap_or(0);
	let pmax = pyramid.level_max().unwrap_or(0);
	let (zmin, zmax) = parse_zoom(args.zoom.as_deref(), pmin, pmax)?;
	let levels: Vec<u8> = (zmin.max(pmin)..=zmax.min(pmax)).collect();

	// Build the scan plan. Without --sample we scan each level's full data bbox.
	// With --sample we read a fixed number of contiguous square windows per level,
	// derived from the deepest (largest) level so shallower levels end up covered
	// more fully — and each window maps to a contiguous file range, keeping remote
	// reads cheap.
	let windows_per_level = sample.map(|fraction| {
		let deepest = levels
			.iter()
			.map(|l| pyramid.level_ref(*l).to_bbox().count_tiles())
			.max()
			.unwrap_or(0);
		windows_for_sample(fraction, deepest)
	});

	let mut plan: Vec<TileBBox> = Vec::new();
	for level in &levels {
		let bbox = pyramid.level_ref(*level).to_bbox();
		if bbox.is_empty() {
			continue;
		}
		match windows_per_level {
			None => plan.push(bbox),
			Some(k) => plan.extend(plan_windows(*level, &bbox, k, WINDOW_SIZE)?),
		}
	}
	let window_count = plan.len();

	let total: u64 = plan.iter().map(TileBBox::count_tiles).sum();
	let progress = runtime.create_progress("Checking shortbread conformance", total);

	let analyzed = Arc::new(AtomicU64::new(0));
	let mut registry = Registry::new();

	for window in plan {
		let progress = progress.clone();
		let schema = Arc::clone(&schema);
		let analyzed = Arc::clone(&analyzed);
		let mut stream = reader
			.tile_stream(window)
			.await?
			.filter_map_parallel(move |coord, tile| {
				progress.inc(1);
				analyzed.fetch_add(1, Ordering::Relaxed);
				match tile.into_vector() {
					Ok(vt) => Some(validate::analyze_tile(coord, &vt, &schema)),
					Err(e) => {
						log::warn!("skipping tile {coord:?}: {e:#}");
						None
					}
				}
			});

		while let Some((_coord, issues)) = stream.next().await {
			registry.merge(issues);
		}
	}

	progress.finish();

	let body = match args.format {
		OutputFormat::Summary => report::render_summary(&registry, &schema.version, args.min_severity, args.strict),
		OutputFormat::List => report::render_list(&registry, &schema.version, args.min_severity, args.strict),
		OutputFormat::Json => report::render_json(&registry, &schema.version, args.min_severity, args.strict),
	};
	print!("{body}");

	if !matches!(args.format, OutputFormat::Json) {
		if !body.ends_with('\n') {
			println!();
		}
		let (errors, warnings, hints) = registry.histogram(args.strict);
		println!("\nsummary: {errors} error(s) · {warnings} warning(s) · {hints} hint(s)");
		if let Some(percent) = args.sample {
			let n = analyzed.load(Ordering::Relaxed);
			println!("sampled {n} tiles via {window_count} window(s) (~{percent}% of the deepest level)");
		}
	}

	let failing = registry.count_at_or_above(args.fail_on, args.strict);
	if failing > 0 {
		bail!(
			"shortbread check failed: {failing} issue group(s) at or above `{}`",
			args.fail_on.label()
		);
	}
	Ok(())
}

/// Validates `--sample` and converts a percentage to a `(0, 1]` fraction.
/// `None` (no flag) means scan every tile.
fn parse_sample(percent: Option<f64>) -> Result<Option<f64>> {
	match percent {
		None => Ok(None),
		Some(p) if p.is_finite() && p > 0.0 && p <= 100.0 => Ok(Some(p / 100.0)),
		Some(p) => bail!("--sample must be in the range (0, 100], got {p}"),
	}
}

/// Side length (in tiles) of each sampling window. 64 keeps a window well inside
/// a single 256×256 versatiles block, so it maps to one coalesced range read.
const WINDOW_SIZE: u32 = 64;

/// Splitmix64 finalizer — cheap, well-distributed avalanche mixing.
fn mix64(mut z: u64) -> u64 {
	z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
	z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
	z ^ (z >> 31)
}

/// How many windows per level approximate `fraction` of the deepest level (the
/// one with the most tiles). Applying this same count to every level means
/// shallower, smaller levels are sampled more fully. Always at least one.
#[allow(
	clippy::cast_precision_loss,
	clippy::cast_possible_truncation,
	clippy::cast_sign_loss
)]
fn windows_for_sample(fraction: f64, deepest_tiles: u64) -> u32 {
	let target = (fraction * deepest_tiles as f64).ceil() as u64;
	let per_window = u64::from(WINDOW_SIZE) * u64::from(WINDOW_SIZE);
	u32::try_from(target.div_ceil(per_window).max(1)).unwrap_or(u32::MAX)
}

/// Picks up to `k` deterministic square windows of side `s` inside `bbox`. When
/// `k` such windows would already cover the level, returns the level bbox unsplit
/// — small low-zoom levels get scanned whole. Placement is hash-based, so the
/// same level always yields the same windows.
fn plan_windows(level: u8, bbox: &TileBBox, k: u32, s: u32) -> Result<Vec<TileBBox>> {
	let (cols, rows) = (bbox.width(), bbox.height());
	let (sw, sh) = (s.min(cols), s.min(rows));
	if u64::from(k) * u64::from(sw) * u64::from(sh) >= bbox.count_tiles() {
		return Ok(vec![*bbox]);
	}
	let (x_min, y_min) = (bbox.x_min()?, bbox.y_min()?);
	let (span_x, span_y) = (cols - sw + 1, rows - sh + 1);

	// Collect distinct top-left corners. The bounded loop avoids spinning when the
	// placement space is small and hashes collide.
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

/// Parses the `--zoom` argument (`"N"` or `"A-B"`), defaulting to the pyramid's
/// full range.
fn parse_zoom(spec: Option<&str>, default_min: u8, default_max: u8) -> Result<(u8, u8)> {
	let Some(spec) = spec else {
		return Ok((default_min, default_max));
	};
	let parse = |s: &str| s.trim().parse::<u8>().with_context(|| format!("invalid zoom '{s}'"));
	if let Some((a, b)) = spec.split_once('-') {
		let (a, b) = (parse(a)?, parse(b)?);
		if a > b {
			bail!("invalid zoom range '{spec}': start is greater than end");
		}
		Ok((a, b))
	} else {
		let z = parse(spec)?;
		Ok((z, z))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles::runtime::create_test_runtime;

	fn args(input: &str) -> CheckShortbread {
		CheckShortbread {
			input: input.to_string(),
			schema: SchemaVersion::Auto,
			zoom: None,
			sample: None,
			format: OutputFormat::Summary,
			min_severity: Severity::Hint,
			strict: false,
			fail_on: Severity::Error,
		}
	}

	#[tokio::test]
	async fn scans_berlin_fixture() {
		let runtime = create_test_runtime();
		// The Berlin fixture is real shortbread data; a non-strict check with
		// fail_on=error must complete without returning an error. Bound the scan
		// to the low zooms and a sampled subset to keep the test fast.
		let mut a = args("../testdata/berlin.mbtiles");
		a.zoom = Some("0-9".to_string());
		a.sample = Some(25.0);
		run(&a, &runtime).await.expect("check should pass on shortbread data");
	}

	#[tokio::test]
	async fn rejects_raster_input() {
		let runtime = create_test_runtime();
		let result = run(&args("../testdata/berlin.mbtiles.does-not-exist"), &runtime).await;
		assert!(result.is_err());
	}

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
		// 100% of the deepest level → enough windows to tile it (16384 tiles at
		// 64×64 = 4096 per window → 4 windows).
		assert_eq!(windows_for_sample(1.0, 16_384), 4);
		// 10% → ceil(1638.4 / 4096) = 1.
		assert_eq!(windows_for_sample(0.1, 16_384), 1);
		// Larger level → more windows.
		assert_eq!(windows_for_sample(0.1, 4_000_000), 98);
		// Always at least one window, even for a tiny fraction.
		assert_eq!(windows_for_sample(0.001, 1), 1);
	}

	#[test]
	fn plan_windows_is_deterministic_and_bounded() {
		// A level far larger than k windows: expect exactly k distinct windows.
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
		// 8×8 level with 4 windows of 64×64 → windows would cover it all, so the
		// whole bbox is returned unsplit.
		let bbox = TileBBox::from_min_and_max(3, 0, 0, 7, 7).unwrap();
		let plan = plan_windows(3, &bbox, 4, WINDOW_SIZE).unwrap();
		assert_eq!(plan, vec![bbox]);
	}

	#[test]
	fn parse_zoom_forms() {
		assert_eq!(parse_zoom(None, 0, 14).unwrap(), (0, 14));
		assert_eq!(parse_zoom(Some("7"), 0, 14).unwrap(), (7, 7));
		assert_eq!(parse_zoom(Some("3-9"), 0, 14).unwrap(), (3, 9));
		assert!(parse_zoom(Some("9-3"), 0, 14).is_err());
		assert!(parse_zoom(Some("x"), 0, 14).is_err());
	}
}
