//! `versatiles probe` — render a container analysis for the terminal.
//!
//! The analysis itself lives in [`versatiles_container::probe`] and returns a
//! [`ProbeReport`]. This module only turns that value into `PrettyPrint` output,
//! so the same answers are reachable from a library, a server or a test without
//! going through rendered text.

use anyhow::Result;
use versatiles_container::{
	ContentsReport, LayerSizeEntry, ProbeReport, TileSizeReport, TileSource, TilesRuntime, VectorContentsReport,
	probe::sampling::parse_sample, probe_report,
};
use versatiles_core::{ProbeDepth, utils::PrettyPrint};
use versatiles_geometry::vector_tile::LayerStats;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// Tile container to probe (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(required = true, verbatim_doc_comment)]
	filename: String,

	/// deep scan (depending on the container implementation)
	///   -d: scans container metadata
	///  -dd: scans all tile sizes
	/// -ddd: scans all tile contents
	#[arg(long, short, action = clap::ArgAction::Count, verbatim_doc_comment)]
	deep: u8,

	/// Sample only a portion of tiles during `-ddd` content scanning.
	/// PERCENT (0–100) is the approximate share of the deepest zoom level to
	/// read; shallower levels are covered more fully. Tiles are read in
	/// contiguous square windows so remote sources need fewer range requests.
	/// Example: `--sample 10` reads roughly 10% of the deepest zoom level.
	#[arg(long, value_name = "PERCENT", verbatim_doc_comment)]
	sample: Option<f64>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	log::info!("probe {:?}", arguments.filename);

	let sample = parse_sample(arguments.sample)?;
	let reader = runtime.reader_from_str(&arguments.filename).await?;

	let level = match arguments.deep {
		0 => ProbeDepth::Shallow,
		1 => ProbeDepth::Container,
		2 => ProbeDepth::TileSizes,
		3..=255 => ProbeDepth::TileContents,
	};

	log::debug!("probing {:?} at depth {:?}", arguments.filename, level);
	probe(&*reader, level, runtime, sample).await?;

	Ok(())
}

/// Analyses `source` and renders the result to a fresh `PrettyPrint` reporter.
///
/// Everything except the container-specific section comes from a single
/// [`probe_report`] call; the container section is still delegated to
/// [`TileSource::probe_container`], which writes to the reporter directly
/// because it is implemented per container format.
pub async fn probe(
	source: &dyn TileSource,
	level: ProbeDepth,
	runtime: &TilesRuntime,
	sample: Option<f64>,
) -> Result<()> {
	use ProbeDepth::{Container, TileContents, TileSizes};

	let report = probe_report(source, level, runtime, sample).await?;
	let mut print = PrettyPrint::new();

	let cat = print.category("meta_data").await;
	cat.add_key_value("source_type", &report.source_type).await;
	cat.add_key_json("meta", &report.tilejson.as_json_value()).await;

	print_metadata(&report, &mut print.category("parameters").await).await;

	if matches!(level, Container | TileSizes | TileContents) {
		log::debug!("probing source {:?} at depth {:?}", source.source_type(), level);
		source
			.probe_container(&mut print.category("container").await, runtime)
			.await?;
	}

	if let Some(sizes) = &report.tile_sizes {
		print_tile_sizes(sizes, &mut print.category("tiles").await).await;
	}

	if let Some(contents) = &report.contents {
		print_contents(contents, &mut print.category("tile contents").await).await;
	}

	Ok(())
}

/// Writes the tile pyramid, compression and format.
async fn print_metadata(report: &ProbeReport, print: &mut PrettyPrint) {
	let rows: Vec<Vec<String>> = report
		.pyramid
		.iter()
		.map(|level| {
			vec![
				format!("{}", level.level),
				format_integer_str(&level.x_min.to_string()),
				format_integer_str(&level.x_max.to_string()),
				format_integer_str(&level.y_min.to_string()),
				format_integer_str(&level.y_max.to_string()),
				format_integer_str(&level.tiles.to_string()),
				format!("{}%", level.coverage_percent),
			]
		})
		.collect();
	print
		.add_table(
			"tile_pyramid",
			&["level", "x0", "x1", "y0", "y1", "tiles", "coverage"],
			&rows,
		)
		.await;
	print.add_key_value("tile compression", &report.tile_compression).await;
	print.add_key_value("tile format", &report.tile_format).await;
	// Only shown when the TileJSON declares nothing — and labelled, because this
	// one was read off a tile rather than claimed by the source (issue #247).
	if let Some(size) = report.measured_tile_size {
		print.add_key_value("tile size in px (measured)", &size.size()).await;
	}
}

/// Formats a `u64` with underscores as thousands separators (e.g. `1_234_567`).
fn format_integer_str(v: &str) -> String {
	let mut result = String::new();
	for (i, c) in v.chars().enumerate() {
		if i > 0 && (v.len() - i).is_multiple_of(3) {
			result.push('_');
		}
		result.push(c);
	}
	result
}

/// Writes average size and the top-10 biggest tiles.
async fn print_tile_sizes(sizes: &TileSizeReport, print: &mut PrettyPrint) {
	if sizes.tile_count == 0 {
		print.add_warning("no tiles found").await;
		return;
	}

	print.add_key_value("tile count", &sizes.tile_count).await;
	print.add_key_value("average tile size", &sizes.average_size()).await;

	let rows: Vec<Vec<String>> = sizes
		.biggest_tiles
		.iter()
		.enumerate()
		.map(|(i, e)| {
			vec![
				format!("{}", i + 1),
				format!("{}", e.z),
				format!("{}", e.x),
				format!("{}", e.y),
				format_integer_str(&e.size.to_string()),
			]
		})
		.collect();
	print
		.add_table("biggest tiles", &["#", "z", "x", "y", "size"], &rows)
		.await;

	let rows: Vec<Vec<String>> = sizes
		.per_level
		.iter()
		.map(|level| {
			vec![
				format!("{}", level.level),
				format_integer_str(&level.count.to_string()),
				format_integer_str(&level.size_sum.to_string()),
				format_integer_str(&level.average_size().to_string()),
			]
		})
		.collect();
	print
		.add_table(
			"tile size analysis per level",
			&["level", "count", "size_sum", "avg_size"],
			&rows,
		)
		.await;
}

async fn print_contents(contents: &ContentsReport, print: &mut PrettyPrint) {
	match contents {
		ContentsReport::Unsupported => {
			print
				.add_warning("deep tile contents probing is only implemented for vector sources")
				.await;
		}
		ContentsReport::Vector(report) => {
			if let Some(percent) = report.sample_fraction.map(|f| f * 100.0) {
				print
					.add_key_value("sampling", &format!("~{percent:.0}% of deepest zoom level"))
					.await;
			}
			print_validation_summary(print, report).await;
			print_size_breakdown(print, report).await;
		}
	}
}

/// Prints the container-wide uncompressed-byte breakdown grouped by zoom level
/// and layer, plus an all-zooms per-layer roll-up. Bytes are the same
/// uncompressed MVT figures `analyze-tile` reports, summed over every tile.
async fn print_size_breakdown(print: &mut PrettyPrint, report: &VectorContentsReport) {
	if report.layer_sizes.is_empty() {
		return;
	}

	let total = report.total_bytes().max(1);
	let show_ids = report.has_feature_ids();

	let mut headers: Vec<&str> = vec!["zoom", "layer", "tiles", "features", "geometry", "tags", "props"];
	if show_ids {
		headers.push("ids");
	}
	headers.extend(["other", "total", "%"]);

	let rows: Vec<Vec<String>> = report
		.layer_sizes
		.iter()
		.map(|entry: &LayerSizeEntry| {
			let mut row = vec![
				format!("{}", entry.zoom),
				entry.layer.clone(),
				format_integer_str(&entry.tiles.to_string()),
			];
			row.extend(size_columns(&entry.stats, show_ids, total));
			row
		})
		.collect();

	print
		.add_table("uncompressed size by zoom × layer", &headers, &rows)
		.await;

	let mut headers: Vec<&str> = vec!["layer", "features", "geometry", "tags", "props"];
	if show_ids {
		headers.push("ids");
	}
	headers.extend(["other", "total", "%"]);

	let rows: Vec<Vec<String>> = report
		.layer_totals()
		.iter()
		.map(|(name, stats)| {
			let mut row = vec![name.clone()];
			row.extend(size_columns(stats, show_ids, total));
			row
		})
		.collect();

	print
		.add_table("uncompressed size by layer (all zooms)", &headers, &rows)
		.await;
}

/// The byte columns both size tables share, from `features` to `%`.
fn size_columns(stats: &LayerStats, show_ids: bool, total: usize) -> Vec<String> {
	let mut row = vec![
		format_integer_str(&stats.feature_count.to_string()),
		format_integer_str(&stats.geometry_bytes.to_string()),
		format_integer_str(&stats.tag_bytes.to_string()),
		format_integer_str(&stats.property_bytes().to_string()),
	];
	if show_ids {
		row.push(format_integer_str(&stats.id_bytes.to_string()));
	}
	row.push(format_integer_str(&stats.other_bytes().to_string()));
	row.push(format_integer_str(&stats.encoded_bytes.to_string()));
	row.push(format!("{}%", stats.encoded_bytes * 100 / total));
	row
}

async fn print_validation_summary(print: &mut PrettyPrint, report: &VectorContentsReport) {
	let counters = &report.issues;
	print.add_key_value("tiles scanned", &report.tiles_scanned).await;

	if counters.decode_failures > 0 {
		print
			.add_warning(&format!(
				"{} tile(s) failed to decode as vector — counted but not validated",
				counters.decode_failures
			))
			.await;
	}

	let total = counters.total_issues();
	if total == 0 {
		print.add_key_value("MVT spec issues", &"none").await;
		return;
	}

	print.add_key_value("MVT spec issues (total)", &total).await;
	print
		.add_key_value("tiles with issues", &counters.tiles_with_issues)
		.await;

	let kind_rows: Vec<Vec<String>> = counters
		.by_kind()
		.into_iter()
		.filter(|(_, n)| *n > 0)
		.map(|(name, n)| vec![name.to_string(), format_integer_str(&n.to_string())])
		.collect();

	print.add_table("issues by kind", &["kind", "count"], &kind_rows).await;

	if !report.samples.is_empty() {
		let rows: Vec<Vec<String>> = report
			.samples
			.iter()
			.map(|s| {
				vec![
					format!("{}", s.z),
					format!("{}", s.x),
					format!("{}", s.y),
					s.layer.clone(),
					s.feature_index.map_or("-".to_string(), |i| i.to_string()),
					s.kind.clone(),
				]
			})
			.collect();
		print
			.add_table(
				&format!("sample issues (first {})", rows.len()),
				&["z", "x", "y", "layer", "feature", "kind"],
				&rows,
			)
			.await;
	}

	// Actionable tip: distinguish issues that vector_repair fixes automatically
	// from those that additionally require drop_offenders=true.
	let tip = match (
		counters.fixable_automatically() > 0,
		counters.needs_drop_offenders() > 0,
	) {
		(true, false) => Some("pipe through `| vector_repair` to fix these issues automatically"),
		(_, true) => Some(
			"pipe through `| vector_repair drop_offenders=true` to fix all issues (unfixable features will be removed)",
		),
		(false, false) => None,
	};
	if let Some(msg) = tip {
		print.add_key_value("fix", &msg).await;
	}
}

#[cfg(test)]
mod tests {
	use versatiles::runtime::create_test_runtime;

	use super::*;
	use crate::tests::run_command;

	#[test]
	fn test_local() -> Result<()> {
		run_command(vec!["versatiles", "probe", "-q", "../testdata/berlin.mbtiles"])?;
		Ok(())
	}

	#[test]
	fn test_remote() -> Result<()> {
		let server = crate::test_http_server::TestHttpServer::shared();
		run_command(vec!["versatiles", "probe", "-q", &server.url("berlin.pmtiles")])?;
		Ok(())
	}

	/// Exercises the full orchestration and each renderer against a real MBTiles
	/// file. The analysis behind them is covered in `versatiles_container::probe`.
	#[tokio::test]
	async fn probe_all_levels_against_mbtiles() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let source: &dyn TileSource = &*reader;

		probe(source, ProbeDepth::Shallow, &runtime, None).await?;
		probe(source, ProbeDepth::Container, &runtime, None).await?;

		let report = probe_report(source, ProbeDepth::TileSizes, &runtime, None).await?;

		let mut printer = PrettyPrint::new();
		print_metadata(&report, &mut printer).await;
		let out = printer.stringify().await;
		assert!(out.contains("tile compression"), "got: {out}");
		assert!(out.contains("tile format"), "got: {out}");

		let mut printer = PrettyPrint::new();
		print_tile_sizes(
			report.tile_sizes.as_ref().expect("TileSizes depth"),
			&mut printer.category("tiles").await,
		)
		.await;
		let out = printer.stringify().await;
		assert!(out.contains("tile count"), "got: {out}");
		assert!(out.contains("biggest tiles"), "got: {out}");
		assert!(out.contains("tile size analysis per level"), "got: {out}");

		Ok(())
	}

	/// Walk every tile in `berlin.mbtiles` through the validator-backed deep
	/// probe and render it, so the rendering path stays exercised end to end.
	#[tokio::test]
	async fn probe_tile_contents_against_mbtiles() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let source: &dyn TileSource = &*reader;

		let report = probe_report(source, ProbeDepth::TileContents, &runtime, None).await?;
		let contents = report.contents.as_ref().expect("TileContents depth");

		let mut printer = PrettyPrint::new();
		print_contents(contents, &mut printer.category("tile contents").await).await;
		let out = printer.stringify().await;
		assert!(out.contains("tiles scanned"), "got: {out}");
		assert!(out.contains("uncompressed size by zoom × layer"), "got: {out}");
		assert!(out.contains("uncompressed size by layer (all zooms)"), "got: {out}");

		Ok(())
	}
}
