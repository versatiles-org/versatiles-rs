//! `versatiles dev validate-schema` — validate a vector-tile container against a
//! tile schema. Currently the only supported schema is
//! [Shortbread](https://shortbread-tiles.org/).
//!
//! Every tile is decoded and checked, per layer/feature/attribute, against an
//! embedded copy of the schema (Shortbread versions 1.0 and 1.1). Findings are
//! aggregated into counted issue groups and printed as a summary, list, or JSON.

mod report;
mod schema;
mod validate;

use crate::tools::tile_sampling::{build_scan_plan, parse_sample};
use anyhow::{Context, Result, bail};
use report::{Registry, Severity};
use schema::{Schema, SchemaSelector};
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
/// Validate that the vector tiles in a container follow a tile schema.
///
/// Reports unknown/missing layers and attributes, wrong value types or
/// geometries, and out-of-vocabulary values. Problems are graded as errors,
/// warnings, or hints; unknown values are tolerated by default (use `--strict`
/// to treat them as hard failures).
pub struct ValidateSchema {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// Schema to validate against: `auto`, `shortbread`, or `shortbread@<version>`
	/// (e.g. `shortbread@1.1`). `auto` guesses the schema and version; a bare family
	/// name (`shortbread`) guesses the version.
	#[arg(long, default_value = "auto", value_parser = schema::parse_schema_selector, verbatim_doc_comment)]
	schema: SchemaSelector,

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

pub async fn run(args: &ValidateSchema, runtime: &TilesRuntime) -> Result<()> {
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

	let level_bboxes = levels.iter().map(|l| pyramid.level_ref(*l).to_bbox());
	let plan = build_scan_plan(level_bboxes, sample)?;
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

	fn args(input: &str) -> ValidateSchema {
		ValidateSchema {
			input: input.to_string(),
			schema: SchemaSelector::Auto,
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
	fn parse_zoom_forms() {
		assert_eq!(parse_zoom(None, 0, 14).unwrap(), (0, 14));
		assert_eq!(parse_zoom(Some("7"), 0, 14).unwrap(), (7, 7));
		assert_eq!(parse_zoom(Some("3-9"), 0, 14).unwrap(), (3, 9));
		assert!(parse_zoom(Some("9-3"), 0, 14).is_err());
		assert!(parse_zoom(Some("x"), 0, 14).is_err());
	}
}
