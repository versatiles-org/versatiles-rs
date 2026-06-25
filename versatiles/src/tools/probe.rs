use crate::tools::tile_breakdown::{LayerStats, layer_stats};
use crate::tools::tile_sampling::{build_scan_plan, parse_sample};
use anyhow::Result;
use std::collections::HashMap;
use versatiles_container::{TileSource, TilesRuntime};
use versatiles_core::TileBBox;
use versatiles_core::{ProbeDepth, TileType, utils::PrettyPrint};
use versatiles_geometry::vector_tile::{DegenerateReason, GeomType, IssueKind, ValidationIssue, validate_tile};

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
	probe(&**reader, level, runtime, sample).await?;

	Ok(())
}

/// Performs a hierarchical CLI probe of `source` at the specified depth.
///
/// Writes metadata, container specifics, tiles, and tile contents to a fresh
/// `PrettyPrint` reporter based on `level`. Format-specific details are
/// delegated to the source via `TileSource::probe_container` and
/// `TileSource::probe_tile_contents`.
pub async fn probe(
	source: &dyn TileSource,
	level: ProbeDepth,
	runtime: &TilesRuntime,
	sample: Option<f64>,
) -> Result<()> {
	use ProbeDepth::{Container, TileContents, TileSizes};

	let mut print = PrettyPrint::new();

	let cat = print.category("meta_data").await;
	cat.add_key_value("source_type", &source.source_type().to_string())
		.await;
	cat.add_key_json("meta", &source.tilejson().as_json_value()).await;

	probe_metadata(source, &mut print.category("parameters").await).await?;

	if matches!(level, Container | TileSizes | TileContents) {
		log::debug!("probing source {:?} at depth {:?}", source.source_type(), level);
		source
			.probe_container(&mut print.category("container").await, runtime)
			.await?;
	}

	if matches!(level, TileSizes | TileContents) {
		log::debug!(
			"probing tiles {:?} at depth {:?}",
			source.tilejson().as_json_value(),
			level
		);
		probe_tile_sizes(source, &mut print.category("tiles").await, runtime).await?;
	}

	if matches!(level, TileContents) {
		log::debug!(
			"probing tile contents {:?} at depth {:?}",
			source.tilejson().as_json_value(),
			level
		);
		probe_tile_contents(source, &mut print.category("tile contents").await, runtime, sample).await?;
	}

	Ok(())
}

/// Writes source metadata (tile pyramid, formats, compression) to `print`.
pub async fn probe_metadata(source: &dyn TileSource, print: &mut PrettyPrint) -> Result<()> {
	let metadata = source.metadata();
	let tile_pyramid = source.tile_pyramid().await?;
	let rows: Vec<Vec<String>> = tile_pyramid
		.iter()
		.filter(|level| !level.is_empty())
		.map(|level| {
			let bbox = level.to_bbox();
			let tiles = level.count_tiles();
			let coverage = tiles * 100 / bbox.count_tiles();
			vec![
				format!("{}", level.level()),
				format_integer_str(&bbox.x_min().unwrap().to_string()),
				format_integer_str(&bbox.x_max().unwrap().to_string()),
				format_integer_str(&bbox.y_min().unwrap().to_string()),
				format_integer_str(&bbox.y_max().unwrap().to_string()),
				format_integer_str(&tiles.to_string()),
				format!("{coverage}%"),
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
	print
		.add_key_value("tile compression", metadata.tile_compression())
		.await;
	print.add_key_value("tile format", metadata.tile_format()).await;
	Ok(())
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

/// Scans all tiles, reporting average size and the top-10 biggest tiles.
#[allow(clippy::too_many_lines)]
pub async fn probe_tile_sizes(source: &dyn TileSource, print: &mut PrettyPrint, runtime: &TilesRuntime) -> Result<()> {
	#[derive(Debug)]
	#[allow(dead_code)]
	struct Entry {
		size: u64,
		x: u32,
		y: u32,
		z: u8,
	}

	let mut biggest_tiles: Vec<Entry> = Vec::new();
	let mut min_size: u64 = 0;
	let mut size_sum: u64 = 0;
	let mut tile_count: u64 = 0;
	let mut level_stats: Vec<(u8, u64, u64)> = Vec::new();

	let tile_pyramid = source.tile_pyramid().await?;
	let total_tiles = tile_pyramid.count_tiles();
	let progress = runtime.create_progress("scanning tiles", total_tiles);

	for bbox in tile_pyramid.to_iter_bboxes().filter(|b| !b.is_empty()) {
		let mut level_size_sum: u64 = 0;
		let mut level_count: u64 = 0;
		let mut stream = source.tile_size_stream(bbox).await?;
		while let Some((coord, size_u32)) = stream.next().await {
			let size = u64::from(size_u32);

			tile_count += 1;
			size_sum += size;
			level_size_sum += size;
			level_count += 1;
			progress.inc(1);

			if size < min_size {
				continue;
			}

			let pos = biggest_tiles
				.binary_search_by(|e| e.size.cmp(&size).reverse())
				.unwrap_or_else(|p| p);
			biggest_tiles.insert(
				pos,
				Entry {
					size,
					x: coord.x,
					y: coord.y,
					z: coord.level,
				},
			);
			if biggest_tiles.len() > 10 {
				biggest_tiles.pop();
			}
			min_size = biggest_tiles.last().expect("biggest_tiles is non-empty").size;
		}
		level_stats.push((bbox.level(), level_count, level_size_sum));
	}
	progress.finish();

	if tile_count > 0 {
		print.add_key_value("tile count", &tile_count).await;
		print
			.add_key_value("average tile size", &size_sum.div_euclid(tile_count))
			.await;

		let rows: Vec<Vec<String>> = biggest_tiles
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

		let rows: Vec<Vec<String>> = level_stats
			.iter()
			.map(|(level, count, size)| {
				let avg = if *count > 0 { size / count } else { 0 };
				vec![
					format!("{level}"),
					format_integer_str(&count.to_string()),
					format_integer_str(&size.to_string()),
					format_integer_str(&avg.to_string()),
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
	} else {
		print.add_warning("no tiles found").await;
	}

	Ok(())
}

/// Walks every tile in `source` and reports MVT spec violations found by the
/// validator. For non-vector sources, emits a "not implemented" warning — we
/// don't have content-level diagnostics for raster yet.
async fn probe_tile_contents(
	source: &dyn TileSource,
	print: &mut PrettyPrint,
	runtime: &TilesRuntime,
	sample: Option<f64>,
) -> Result<()> {
	if source.metadata().tile_format().to_type() != TileType::Vector {
		print
			.add_warning("deep tile contents probing is only implemented for vector sources")
			.await;
		return Ok(());
	}

	probe_mvt_validation(source, print, runtime, sample).await
}

#[derive(Default)]
struct ValidationCounters {
	missing_extent: u64,
	missing_version: u64,
	duplicate_layer_name: u64,
	orphan_inner: u64,
	degenerate_too_few: u64,
	degenerate_sub_pixel: u64,
	degenerate_collinear: u64,
	unknown_geom: u64,
	empty_geom_point: u64,
	empty_geom_line: u64,
	empty_geom_polygon: u64,
	malformed_stream: u64,
	decode_failures: u64,
	tiles_with_issues: u64,
}

impl ValidationCounters {
	fn total_issues(&self) -> u64 {
		self.missing_extent
			+ self.missing_version
			+ self.duplicate_layer_name
			+ self.orphan_inner
			+ self.degenerate_too_few
			+ self.degenerate_sub_pixel
			+ self.degenerate_collinear
			+ self.unknown_geom
			+ self.empty_geom_point
			+ self.empty_geom_line
			+ self.empty_geom_polygon
			+ self.malformed_stream
	}

	fn record(&mut self, kind: &IssueKind) {
		match kind {
			IssueKind::MissingExtent => self.missing_extent += 1,
			IssueKind::MissingVersion => self.missing_version += 1,
			IssueKind::DuplicateLayerName => self.duplicate_layer_name += 1,
			IssueKind::OrphanInnerRing => self.orphan_inner += 1,
			IssueKind::DegenerateRing(DegenerateReason::TooFewVertices) => self.degenerate_too_few += 1,
			IssueKind::DegenerateRing(DegenerateReason::SubPixel) => self.degenerate_sub_pixel += 1,
			IssueKind::DegenerateRing(DegenerateReason::Collinear) => self.degenerate_collinear += 1,
			// `EmptyGeometryForType(Unknown)` is unreachable in practice (the
			// validator filters that case out before recording), but folded
			// into `unknown_geom` so the match stays exhaustive.
			IssueKind::UnknownGeometryType | IssueKind::EmptyGeometryForType(GeomType::Unknown) => self.unknown_geom += 1,
			IssueKind::EmptyGeometryForType(GeomType::MultiPoint) => self.empty_geom_point += 1,
			IssueKind::EmptyGeometryForType(GeomType::MultiLineString) => self.empty_geom_line += 1,
			IssueKind::EmptyGeometryForType(GeomType::MultiPolygon) => self.empty_geom_polygon += 1,
			IssueKind::MalformedCommandStream(_) => self.malformed_stream += 1,
		}
	}
}

const VALIDATION_SAMPLE_LIMIT: usize = 10;

/// Per-tile result of the decode+validate step, produced off-thread so the
/// aggregation loop only has to fold cheap owned values.
enum TileCheck {
	/// The tile could not be decoded as a vector tile.
	DecodeFailed,
	/// The tile decoded; carries the spec violations and the per-layer byte
	/// breakdown for size aggregation.
	Validated {
		issues: Vec<ValidationIssue>,
		layers: Vec<LayerStats>,
	},
}

/// Decodes a single tile, validates it, and computes its per-layer byte
/// breakdown. This is the CPU-bound work (decompress + protobuf parse + geometry
/// validation + re-encode for sizes) that gets fanned out across worker threads
/// by `map_parallel`.
fn check_tile(mut tile: versatiles_container::Tile) -> TileCheck {
	match tile.as_vector() {
		Ok(vt) => TileCheck::Validated {
			issues: validate_tile(vt),
			// Size aggregation is best-effort: if a layer fails to re-encode we
			// skip its sizes rather than aborting the whole deep probe.
			layers: layer_stats(vt).unwrap_or_default(),
		},
		Err(_) => TileCheck::DecodeFailed,
	}
}

/// Aggregated byte breakdown of one layer at one zoom level, across all tiles.
#[derive(Default)]
struct LayerAgg {
	/// How many tiles at this zoom contained this layer.
	tiles: u64,
	stats: LayerStats,
}

/// Iterates every tile, runs the MVT validator and per-layer size breakdown on
/// it, and prints both a validation summary and a zoom × layer size breakdown.
/// The per-tile decode+validate+measure (the expensive part) runs in parallel
/// across worker threads via `map_parallel`; the aggregation stays on this task
/// so it needs no synchronization.
async fn probe_mvt_validation(
	source: &dyn TileSource,
	print: &mut PrettyPrint,
	runtime: &TilesRuntime,
	sample: Option<f64>,
) -> Result<()> {
	let mut counters = ValidationCounters::default();
	let mut samples: Vec<Vec<String>> = Vec::with_capacity(VALIDATION_SAMPLE_LIMIT);
	let mut tile_count: u64 = 0;
	// (zoom, layer name) -> aggregated byte breakdown.
	let mut size_agg: HashMap<(u8, String), LayerAgg> = HashMap::new();

	let tile_pyramid = source.tile_pyramid().await?;
	let plan = build_scan_plan(tile_pyramid.to_iter_bboxes(), sample)?;
	let total_in_plan: u64 = plan.iter().map(TileBBox::count_tiles).sum();
	let progress = runtime.create_progress("validating tile contents", total_in_plan);

	for bbox in plan {
		let mut stream = source
			.tile_stream(bbox)
			.await?
			.map_parallel(|_coord, tile| check_tile(tile));
		while let Some((coord, check)) = stream.next().await {
			tile_count += 1;
			progress.inc(1);

			let (issues, layers) = match check {
				TileCheck::DecodeFailed => {
					counters.decode_failures += 1;
					continue;
				}
				TileCheck::Validated { issues, layers } => (issues, layers),
			};

			for layer in &layers {
				let entry = size_agg.entry((coord.level, layer.name.clone())).or_default();
				entry.tiles += 1;
				entry.stats.add(layer);
			}

			if issues.is_empty() {
				continue;
			}
			counters.tiles_with_issues += 1;

			for issue in &issues {
				counters.record(&issue.kind);
				if samples.len() < VALIDATION_SAMPLE_LIMIT {
					samples.push(vec![
						format!("{}", coord.level),
						format!("{}", coord.x),
						format!("{}", coord.y),
						issue.layer.clone(),
						issue.feature_index.map_or("-".to_string(), |i| i.to_string()),
						describe_kind(&issue.kind),
					]);
				}
			}
		}
	}
	progress.finish();

	if let Some(percent) = sample.map(|f| f * 100.0) {
		print
			.add_key_value("sampling", &format!("~{percent:.0}% of deepest zoom level"))
			.await;
	}

	print_validation_summary(print, tile_count, &counters, &samples).await;
	print_size_breakdown(print, &size_agg).await;
	Ok(())
}

/// Prints the container-wide uncompressed-byte breakdown grouped by zoom level
/// and layer, plus an all-zooms per-layer roll-up. Bytes are the same
/// uncompressed MVT figures `analyze-tile` reports, summed over every tile.
async fn print_size_breakdown(print: &mut PrettyPrint, size_agg: &HashMap<(u8, String), LayerAgg>) {
	if size_agg.is_empty() {
		return;
	}

	let grand_total: usize = size_agg.values().map(|a| a.stats.encoded_bytes).sum();
	let total = grand_total.max(1);

	// Whether any layer carries feature ids — controls the optional `ids` column.
	let show_ids = size_agg.values().any(|a| a.stats.id_bytes > 0);

	// Per zoom × layer, ordered by (zoom asc, total bytes desc).
	let mut entries: Vec<(&(u8, String), &LayerAgg)> = size_agg.iter().collect();
	entries.sort_by(|a, b| {
		a.0.0
			.cmp(&b.0.0)
			.then(b.1.stats.encoded_bytes.cmp(&a.1.stats.encoded_bytes))
	});

	let mut headers: Vec<&str> = vec!["zoom", "layer", "tiles", "features", "geometry", "tags", "props"];
	if show_ids {
		headers.push("ids");
	}
	headers.extend(["other", "total", "%"]);

	let rows: Vec<Vec<String>> = entries
		.iter()
		.map(|((zoom, name), agg)| {
			let s = &agg.stats;
			let mut row = vec![
				format!("{zoom}"),
				name.clone(),
				format_integer_str(&agg.tiles.to_string()),
				format_integer_str(&s.feature_count.to_string()),
				format_integer_str(&s.geometry_bytes.to_string()),
				format_integer_str(&s.tag_bytes.to_string()),
				format_integer_str(&s.property_bytes().to_string()),
			];
			if show_ids {
				row.push(format_integer_str(&s.id_bytes.to_string()));
			}
			row.push(format_integer_str(&s.other_bytes().to_string()));
			row.push(format_integer_str(&s.encoded_bytes.to_string()));
			row.push(format!("{}%", s.encoded_bytes * 100 / total));
			row
		})
		.collect();

	print
		.add_table("uncompressed size by zoom × layer", &headers, &rows)
		.await;

	// All-zooms roll-up: which layer dominates the whole container.
	let mut per_layer: HashMap<&str, LayerStats> = HashMap::new();
	for ((_, name), agg) in size_agg {
		per_layer.entry(name).or_default().add(&agg.stats);
	}
	let mut layer_rows: Vec<(&str, LayerStats)> = per_layer.into_iter().collect();
	layer_rows.sort_by_key(|entry| std::cmp::Reverse(entry.1.encoded_bytes));

	let mut headers: Vec<&str> = vec!["layer", "features", "geometry", "tags", "props"];
	if show_ids {
		headers.push("ids");
	}
	headers.extend(["other", "total", "%"]);

	let rows: Vec<Vec<String>> = layer_rows
		.iter()
		.map(|(name, s)| {
			let mut row = vec![
				(*name).to_string(),
				format_integer_str(&s.feature_count.to_string()),
				format_integer_str(&s.geometry_bytes.to_string()),
				format_integer_str(&s.tag_bytes.to_string()),
				format_integer_str(&s.property_bytes().to_string()),
			];
			if show_ids {
				row.push(format_integer_str(&s.id_bytes.to_string()));
			}
			row.push(format_integer_str(&s.other_bytes().to_string()));
			row.push(format_integer_str(&s.encoded_bytes.to_string()));
			row.push(format!("{}%", s.encoded_bytes * 100 / total));
			row
		})
		.collect();

	print
		.add_table("uncompressed size by layer (all zooms)", &headers, &rows)
		.await;
}

async fn print_validation_summary(
	print: &mut PrettyPrint,
	tile_count: u64,
	counters: &ValidationCounters,
	samples: &[Vec<String>],
) {
	print.add_key_value("tiles scanned", &tile_count).await;

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

	let kind_rows: Vec<Vec<String>> = [
		("MissingExtent", counters.missing_extent),
		("MissingVersion", counters.missing_version),
		("DuplicateLayerName", counters.duplicate_layer_name),
		("OrphanInnerRing", counters.orphan_inner),
		("DegenerateRing(TooFewVertices)", counters.degenerate_too_few),
		("DegenerateRing(SubPixel)", counters.degenerate_sub_pixel),
		("DegenerateRing(Collinear)", counters.degenerate_collinear),
		("UnknownGeometryType", counters.unknown_geom),
		("EmptyGeometryForType(MultiPoint)", counters.empty_geom_point),
		("EmptyGeometryForType(MultiLineString)", counters.empty_geom_line),
		("EmptyGeometryForType(MultiPolygon)", counters.empty_geom_polygon),
		("MalformedCommandStream", counters.malformed_stream),
	]
	.into_iter()
	.filter(|(_, n)| *n > 0)
	.map(|(name, n)| vec![name.to_string(), format_integer_str(&n.to_string())])
	.collect();

	print.add_table("issues by kind", &["kind", "count"], &kind_rows).await;

	if !samples.is_empty() {
		print
			.add_table(
				&format!("sample issues (first {})", samples.len()),
				&["z", "x", "y", "layer", "feature", "kind"],
				samples,
			)
			.await;
	}

	// Actionable tip: distinguish issues that vector_repair fixes automatically
	// from those that additionally require drop_offenders=true.
	let fixable_automatically = counters.missing_extent
		+ counters.missing_version
		+ counters.duplicate_layer_name
		+ counters.orphan_inner
		+ counters.degenerate_too_few
		+ counters.degenerate_sub_pixel
		+ counters.degenerate_collinear;
	let needs_drop_offenders = counters.unknown_geom
		+ counters.empty_geom_point
		+ counters.empty_geom_line
		+ counters.empty_geom_polygon
		+ counters.malformed_stream;

	let tip = match (fixable_automatically > 0, needs_drop_offenders > 0) {
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

fn describe_kind(kind: &IssueKind) -> String {
	match kind {
		IssueKind::MissingExtent => "MissingExtent".to_string(),
		IssueKind::MissingVersion => "MissingVersion".to_string(),
		IssueKind::DuplicateLayerName => "DuplicateLayerName".to_string(),
		IssueKind::OrphanInnerRing => "OrphanInnerRing".to_string(),
		IssueKind::DegenerateRing(reason) => format!("DegenerateRing({reason:?})"),
		IssueKind::UnknownGeometryType => "UnknownGeometryType".to_string(),
		IssueKind::EmptyGeometryForType(geom_type) => format!("EmptyGeometryForType({geom_type:?})"),
		IssueKind::MalformedCommandStream(_) => "MalformedCommandStream".to_string(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::tests::run_command;
	use versatiles::runtime::create_test_runtime;

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

	/// Exercises the full orchestration and each free function against a real
	/// MBTiles file. This replaces the per-method tests that used to live on
	/// `TileSource` before the probe helpers moved out of the trait.
	#[tokio::test]
	async fn probe_all_levels_against_mbtiles() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let source: &dyn TileSource = &**reader;

		probe(source, ProbeDepth::Shallow, &runtime, None).await?;
		probe(source, ProbeDepth::Container, &runtime, None).await?;

		let mut printer = PrettyPrint::new();
		probe_metadata(source, &mut printer).await?;
		let out = printer.stringify().await;
		assert!(out.contains("tile compression"), "got: {out}");
		assert!(out.contains("tile format"), "got: {out}");

		let mut printer = PrettyPrint::new();
		probe_tile_sizes(source, &mut printer.category("tiles").await, &runtime).await?;
		let out = printer.stringify().await;
		assert!(out.contains("tile count"), "got: {out}");
		assert!(out.contains("biggest tiles"), "got: {out}");
		assert!(out.contains("tile size analysis per level"), "got: {out}");

		Ok(())
	}

	/// Walk every tile in `berlin.mbtiles` through the validator-backed deep
	/// probe. The fixture was regenerated through `vector_repair` so it is
	/// Verifies that the probe runs correctly against the berlin.mbtiles fixture and
	/// that the output has the expected structure. The fixture's layers omit the
	/// `extent` field (relying on the proto default of 4096), which Phase 2 of the
	/// MVT validator now correctly flags as `MissingExtent`. Phase 3 (repair) will
	/// address this in the test fixture.
	#[tokio::test]
	async fn probe_tile_contents_against_mbtiles_reports_no_issues() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let source: &dyn TileSource = &**reader;

		let mut printer = PrettyPrint::new();
		probe_tile_contents(source, &mut printer.category("tile contents").await, &runtime, None).await?;
		let out = printer.stringify().await;

		assert!(out.contains("tiles scanned"), "missing tile count: {out}");
		assert!(out.contains("MVT spec issues"), "missing validator section: {out}");
		// berlin.mbtiles layers omit the `extent` field — the validator correctly
		// reports MissingExtent for every layer of every tile.
		assert!(
			out.contains("MissingExtent"),
			"probe must report MissingExtent for the berlin fixture: {out}"
		);
		// The validator-level fix tip should point to vector_repair.
		assert!(
			out.contains("vector_repair"),
			"probe must suggest vector_repair when structural issues are found: {out}"
		);
		// No geometry-level issues (winding, degenerate rings, etc.) are expected.
		assert!(!out.contains("OrphanInnerRing"), "unexpected geometry issues: {out}");
		assert!(!out.contains("DegenerateRing"), "unexpected geometry issues: {out}");
		assert!(
			!out.contains("MalformedCommandStream"),
			"unexpected geometry issues: {out}"
		);
		// The deep probe also emits the container-wide size breakdown.
		assert!(
			out.contains("uncompressed size by zoom × layer"),
			"missing zoom × layer breakdown: {out}"
		);
		assert!(
			out.contains("uncompressed size by layer (all zooms)"),
			"missing per-layer roll-up: {out}"
		);
		// A known shortbread layer should appear in the breakdown.
		assert!(
			out.contains("place_labels"),
			"expected a known layer in breakdown: {out}"
		);
		Ok(())
	}

	// ── pure-helper unit tests ────────────────────────────────────────────

	#[test]
	fn describe_kind_covers_every_issue_variant() {
		assert_eq!(describe_kind(&IssueKind::MissingExtent), "MissingExtent");
		assert_eq!(describe_kind(&IssueKind::MissingVersion), "MissingVersion");
		assert_eq!(describe_kind(&IssueKind::DuplicateLayerName), "DuplicateLayerName");
		assert_eq!(describe_kind(&IssueKind::OrphanInnerRing), "OrphanInnerRing");
		assert_eq!(describe_kind(&IssueKind::UnknownGeometryType), "UnknownGeometryType");
		assert_eq!(
			describe_kind(&IssueKind::MalformedCommandStream("anything".into())),
			"MalformedCommandStream"
		);
		assert_eq!(
			describe_kind(&IssueKind::DegenerateRing(DegenerateReason::TooFewVertices)),
			"DegenerateRing(TooFewVertices)"
		);
		assert_eq!(
			describe_kind(&IssueKind::DegenerateRing(DegenerateReason::SubPixel)),
			"DegenerateRing(SubPixel)"
		);
		assert_eq!(
			describe_kind(&IssueKind::DegenerateRing(DegenerateReason::Collinear)),
			"DegenerateRing(Collinear)"
		);
		assert_eq!(
			describe_kind(&IssueKind::EmptyGeometryForType(GeomType::MultiPoint)),
			"EmptyGeometryForType(MultiPoint)"
		);
		assert_eq!(
			describe_kind(&IssueKind::EmptyGeometryForType(GeomType::MultiLineString)),
			"EmptyGeometryForType(MultiLineString)"
		);
		assert_eq!(
			describe_kind(&IssueKind::EmptyGeometryForType(GeomType::MultiPolygon)),
			"EmptyGeometryForType(MultiPolygon)"
		);
	}

	#[test]
	fn validation_counters_record_increments_the_matching_field() {
		let mut c = ValidationCounters::default();
		c.record(&IssueKind::MissingExtent);
		c.record(&IssueKind::MissingVersion);
		c.record(&IssueKind::DuplicateLayerName);
		c.record(&IssueKind::OrphanInnerRing);
		c.record(&IssueKind::OrphanInnerRing);
		c.record(&IssueKind::DegenerateRing(DegenerateReason::TooFewVertices));
		c.record(&IssueKind::DegenerateRing(DegenerateReason::SubPixel));
		c.record(&IssueKind::DegenerateRing(DegenerateReason::Collinear));
		c.record(&IssueKind::UnknownGeometryType);
		c.record(&IssueKind::EmptyGeometryForType(GeomType::MultiPoint));
		c.record(&IssueKind::EmptyGeometryForType(GeomType::MultiLineString));
		c.record(&IssueKind::EmptyGeometryForType(GeomType::MultiPolygon));
		c.record(&IssueKind::MalformedCommandStream("err".into()));

		assert_eq!(c.missing_extent, 1);
		assert_eq!(c.missing_version, 1);
		assert_eq!(c.duplicate_layer_name, 1);
		assert_eq!(c.orphan_inner, 2);
		assert_eq!(c.degenerate_too_few, 1);
		assert_eq!(c.degenerate_sub_pixel, 1);
		assert_eq!(c.degenerate_collinear, 1);
		assert_eq!(c.unknown_geom, 1);
		assert_eq!(c.empty_geom_point, 1);
		assert_eq!(c.empty_geom_line, 1);
		assert_eq!(c.empty_geom_polygon, 1);
		assert_eq!(c.malformed_stream, 1);
		assert_eq!(c.total_issues(), 13);
	}

	#[test]
	fn validation_counters_total_is_zero_by_default() {
		let c = ValidationCounters::default();
		assert_eq!(c.total_issues(), 0);
	}

	// ── print_validation_summary output paths ─────────────────────────────

	#[tokio::test]
	async fn print_validation_summary_clean_reports_none() {
		let mut printer = PrettyPrint::new();
		let counters = ValidationCounters::default();
		print_validation_summary(&mut printer, 42, &counters, &[]).await;
		let out = printer.stringify().await;
		assert!(out.contains("tiles scanned: 42"), "got: {out}");
		assert!(out.contains("MVT spec issues"), "got: {out}");
		assert!(out.contains("none"), "got: {out}");
		assert!(!out.contains("issues by kind"), "got: {out}");
	}

	#[tokio::test]
	async fn print_validation_summary_dirty_reports_kind_table_and_samples() {
		let mut printer = PrettyPrint::new();
		let counters = ValidationCounters {
			missing_extent: 0,
			missing_version: 0,
			duplicate_layer_name: 0,
			orphan_inner: 5,
			degenerate_too_few: 0,
			degenerate_sub_pixel: 1,
			degenerate_collinear: 2,
			unknown_geom: 0,
			empty_geom_point: 0,
			empty_geom_line: 0,
			empty_geom_polygon: 0,
			malformed_stream: 1,
			decode_failures: 0,
			tiles_with_issues: 7,
		};
		let samples = vec![vec![
			"14".to_string(),
			"8800".to_string(),
			"5377".to_string(),
			"land".to_string(),
			"3".to_string(),
			"OrphanInnerRing".to_string(),
		]];
		print_validation_summary(&mut printer, 130, &counters, &samples).await;
		let out = printer.stringify().await;

		assert!(out.contains("MVT spec issues (total): 9"), "got: {out}");
		assert!(out.contains("tiles with issues: 7"), "got: {out}");
		assert!(out.contains("issues by kind"), "got: {out}");
		assert!(out.contains("OrphanInnerRing"), "got: {out}");
		assert!(out.contains("DegenerateRing(SubPixel)"), "got: {out}");
		assert!(out.contains("DegenerateRing(Collinear)"), "got: {out}");
		assert!(out.contains("MalformedCommandStream"), "got: {out}");
		// Zero-count kinds must not appear in the table.
		assert!(!out.contains("DegenerateRing(TooFewVertices)"), "got: {out}");
		assert!(!out.contains("UnknownGeometryType"), "got: {out}");
		// Sample table
		assert!(out.contains("sample issues (first 1)"), "got: {out}");
		assert!(out.contains("land"), "got: {out}");
		// Has both fixable (orphan rings) and needs-drop-offenders (malformed stream)
		// → tip should mention drop_offenders=true.
		assert!(
			out.contains("drop_offenders=true"),
			"expected drop_offenders tip: {out}"
		);
	}

	#[tokio::test]
	async fn print_validation_summary_decode_failures_warn() {
		let mut printer = PrettyPrint::new();
		let counters = ValidationCounters {
			decode_failures: 3,
			..ValidationCounters::default()
		};
		print_validation_summary(&mut printer, 10, &counters, &[]).await;
		let out = printer.stringify().await;
		assert!(out.contains("3 tile(s) failed to decode"), "got: {out}");
		// total_issues is still 0 so we still report "none" for spec issues.
		assert!(out.contains("MVT spec issues"), "got: {out}");
		assert!(out.contains("none"), "got: {out}");
	}

	#[tokio::test]
	async fn print_validation_summary_structural_issues_show_basic_fix_tip() {
		let mut printer = PrettyPrint::new();
		let counters = ValidationCounters {
			missing_extent: 5,
			missing_version: 2,
			..ValidationCounters::default()
		};
		print_validation_summary(&mut printer, 10, &counters, &[]).await;
		let out = printer.stringify().await;
		assert!(out.contains("vector_repair"), "expected fix tip: {out}");
		// Structural-only issues don't require drop_offenders.
		assert!(
			!out.contains("drop_offenders=true"),
			"structural issues should not require drop_offenders: {out}"
		);
	}

	#[tokio::test]
	async fn print_validation_summary_clean_shows_no_fix_tip() {
		let mut printer = PrettyPrint::new();
		let counters = ValidationCounters::default();
		print_validation_summary(&mut printer, 5, &counters, &[]).await;
		let out = printer.stringify().await;
		assert!(!out.contains("fix:"), "clean tiles should not show fix tip: {out}");
		assert!(
			!out.contains("vector_repair"),
			"clean tiles should not show fix tip: {out}"
		);
	}

	// ── probe_tile_contents on a raster source ────────────────────────────

	#[tokio::test]
	async fn probe_tile_contents_on_raster_emits_not_implemented_warning() -> Result<()> {
		use versatiles_container::{MockReader, MockReaderProfile};
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?;
		let source: &dyn TileSource = &reader;
		let runtime = create_test_runtime();

		let mut printer = PrettyPrint::new();
		probe_tile_contents(source, &mut printer.category("tile contents").await, &runtime, None).await?;
		let out = printer.stringify().await;
		assert!(
			out.contains("only implemented for vector sources"),
			"expected 'not implemented' warning, got: {out}",
		);
		// And we should NOT have walked any tiles for a raster source.
		assert!(!out.contains("tiles scanned"), "got: {out}");
		Ok(())
	}

	// ── sampling mode ────────────────────────────────────────────────────

	#[tokio::test]
	async fn probe_tile_contents_with_sample_reports_sampling_note() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let source: &dyn TileSource = &**reader;

		let mut printer = PrettyPrint::new();
		// 10% sample — just enough to exercise the path without scanning everything.
		probe_tile_contents(
			source,
			&mut printer.category("tile contents").await,
			&runtime,
			Some(0.1),
		)
		.await?;
		let out = printer.stringify().await;

		assert!(out.contains("sampling"), "expected sampling note in output: {out}");
		assert!(out.contains("tiles scanned"), "expected tile count: {out}");
		assert!(out.contains("MVT spec issues"), "expected validator section: {out}");
		Ok(())
	}

	#[test]
	fn run_with_sample_flag_does_not_error() -> Result<()> {
		run_command(vec![
			"versatiles",
			"probe",
			"-q",
			"-ddd",
			"--sample",
			"25",
			"../testdata/berlin.mbtiles",
		])?;
		Ok(())
	}

	// ── probe_tile_sizes on an empty pyramid ──────────────────────────────

	#[tokio::test]
	async fn probe_tile_sizes_no_tiles_warns() -> Result<()> {
		use versatiles_container::{MockReader, TileSourceMetadata, Traversal};
		use versatiles_core::{TileCompression, TileFormat, TilePyramid};
		let pyramid = TilePyramid::new_empty();
		let metadata = TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None);
		let reader = MockReader::new_mock(pyramid, metadata)?;
		let source: &dyn TileSource = &reader;
		let runtime = create_test_runtime();

		let mut printer = PrettyPrint::new();
		probe_tile_sizes(source, &mut printer.category("tiles").await, &runtime).await?;
		let out = printer.stringify().await;
		assert!(out.contains("no tiles found"), "got: {out}");
		Ok(())
	}

	// ── probe() dispatch at every depth ───────────────────────────────────

	#[tokio::test]
	async fn probe_dispatches_at_each_depth() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let source: &dyn TileSource = &**reader;

		// Each depth level exercises a different branch of the matches!() arms
		// in `probe()` and the `run()` ProbeDepth match. Shallow is already
		// covered by `probe_all_levels_against_mbtiles`; this test fills the
		// other three.
		probe(source, ProbeDepth::Container, &runtime, None).await?;
		probe(source, ProbeDepth::TileSizes, &runtime, None).await?;
		probe(source, ProbeDepth::TileContents, &runtime, None).await?;
		Ok(())
	}

	#[test]
	fn run_at_each_deep_level_dispatches() -> Result<()> {
		// Exercises lines 30..=32 of `run()` (the `-d`, `-dd`, `-ddd` arms of
		// the ProbeDepth match). `-q` suppresses logging so the assertion
		// boils down to "doesn't error out".
		for flag in ["-d", "-dd", "-ddd"] {
			run_command(vec!["versatiles", "probe", "-q", flag, "../testdata/berlin.mbtiles"])?;
		}
		Ok(())
	}
}
