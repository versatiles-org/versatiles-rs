use anyhow::Result;
use versatiles_container::{TileSource, TilesRuntime};
use versatiles_core::{ProbeDepth, TileType, utils::PrettyPrint};
use versatiles_geometry::vector_tile::{DegenerateReason, GeomType, IssueKind, validate_tile};

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
}

#[tokio::main]
pub async fn run(arguments: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	log::info!("probe {:?}", arguments.filename);

	let reader = runtime.reader_from_str(&arguments.filename).await?;

	let level = match arguments.deep {
		0 => ProbeDepth::Shallow,
		1 => ProbeDepth::Container,
		2 => ProbeDepth::TileSizes,
		3..=255 => ProbeDepth::TileContents,
	};

	log::debug!("probing {:?} at depth {:?}", arguments.filename, level);
	probe(&**reader, level, runtime).await?;

	Ok(())
}

/// Performs a hierarchical CLI probe of `source` at the specified depth.
///
/// Writes metadata, container specifics, tiles, and tile contents to a fresh
/// `PrettyPrint` reporter based on `level`. Format-specific details are
/// delegated to the source via `TileSource::probe_container` and
/// `TileSource::probe_tile_contents`.
pub async fn probe(source: &dyn TileSource, level: ProbeDepth, runtime: &TilesRuntime) -> Result<()> {
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
		probe_tile_contents(source, &mut print.category("tile contents").await, runtime).await?;
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
async fn probe_tile_contents(source: &dyn TileSource, print: &mut PrettyPrint, runtime: &TilesRuntime) -> Result<()> {
	if source.metadata().tile_format().to_type() != TileType::Vector {
		print
			.add_warning("deep tile contents probing is only implemented for vector sources")
			.await;
		return Ok(());
	}

	probe_mvt_validation(source, print, runtime).await
}

#[derive(Default)]
struct ValidationCounters {
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
		self.orphan_inner
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

/// Iterates every tile, runs the MVT validator on it, and prints a summary
/// of any issues found. Total cost is one decode-per-tile; with the validator
/// running in lockstep that's cheap relative to the decode itself.
async fn probe_mvt_validation(source: &dyn TileSource, print: &mut PrettyPrint, runtime: &TilesRuntime) -> Result<()> {
	let mut counters = ValidationCounters::default();
	let mut samples: Vec<Vec<String>> = Vec::with_capacity(VALIDATION_SAMPLE_LIMIT);
	let mut tile_count: u64 = 0;

	let tile_pyramid = source.tile_pyramid().await?;
	let total_tiles = tile_pyramid.count_tiles();
	let progress = runtime.create_progress("validating tile contents", total_tiles);

	for bbox in tile_pyramid.to_iter_bboxes().filter(|b| !b.is_empty()) {
		let mut stream = source.tile_stream(bbox).await?;
		while let Some((coord, mut tile)) = stream.next().await {
			tile_count += 1;
			progress.inc(1);

			let Ok(vt) = tile.as_vector() else {
				counters.decode_failures += 1;
				continue;
			};

			let issues = validate_tile(vt);
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
						format!("{}", issue.feature_index),
						describe_kind(&issue.kind),
					]);
				}
			}
		}
	}
	progress.finish();

	print_validation_summary(print, tile_count, &counters, &samples).await;
	Ok(())
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
}

fn describe_kind(kind: &IssueKind) -> String {
	match kind {
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

		probe(source, ProbeDepth::Shallow, &runtime).await?;
		probe(source, ProbeDepth::Container, &runtime).await?;

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
	/// MVT 2.1 conformant; the test asserts the validator reports zero
	/// issues and the output structure is well-formed.
	#[tokio::test]
	async fn probe_tile_contents_against_mbtiles_reports_no_issues() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let source: &dyn TileSource = &**reader;

		let mut printer = PrettyPrint::new();
		probe_tile_contents(source, &mut printer.category("tile contents").await, &runtime).await?;
		let out = printer.stringify().await;

		assert!(out.contains("tiles scanned"), "missing tile count: {out}");
		assert!(out.contains("MVT spec issues"), "missing validator section: {out}");
		assert!(
			out.contains("none"),
			"expected zero issues for repaired fixture, got: {out}"
		);
		Ok(())
	}

	// ── pure-helper unit tests ────────────────────────────────────────────

	#[test]
	fn describe_kind_covers_every_issue_variant() {
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

		assert_eq!(c.orphan_inner, 2);
		assert_eq!(c.degenerate_too_few, 1);
		assert_eq!(c.degenerate_sub_pixel, 1);
		assert_eq!(c.degenerate_collinear, 1);
		assert_eq!(c.unknown_geom, 1);
		assert_eq!(c.empty_geom_point, 1);
		assert_eq!(c.empty_geom_line, 1);
		assert_eq!(c.empty_geom_polygon, 1);
		assert_eq!(c.malformed_stream, 1);
		assert_eq!(c.total_issues(), 10);
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

	// ── probe_tile_contents on a raster source ────────────────────────────

	#[tokio::test]
	async fn probe_tile_contents_on_raster_emits_not_implemented_warning() -> Result<()> {
		use versatiles_container::{MockReader, MockReaderProfile};
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png)?;
		let source: &dyn TileSource = &reader;
		let runtime = create_test_runtime();

		let mut printer = PrettyPrint::new();
		probe_tile_contents(source, &mut printer.category("tile contents").await, &runtime).await?;
		let out = printer.stringify().await;
		assert!(
			out.contains("only implemented for vector sources"),
			"expected 'not implemented' warning, got: {out}",
		);
		// And we should NOT have walked any tiles for a raster source.
		assert!(!out.contains("tiles scanned"), "got: {out}");
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
		probe(source, ProbeDepth::Container, &runtime).await?;
		probe(source, ProbeDepth::TileSizes, &runtime).await?;
		probe(source, ProbeDepth::TileContents, &runtime).await?;
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
