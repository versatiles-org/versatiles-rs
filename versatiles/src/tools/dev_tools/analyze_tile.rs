//! Per-tile size analysis for vector containers.
//!
//! `probe -dd` tells you *which* tiles are huge (the "biggest tiles" table);
//! this tool tells you *why*. Given one or more tile coordinates (`z/x/y`, with
//! optional per-axis ranges) — or, with no coordinate, the N biggest tiles in
//! the container — it decodes each tile and reports a full byte breakdown:
//!
//! - per layer: feature count, vertex count, and the split between geometry,
//!   per-feature tag references, and the property (key+value) table;
//! - a geometry-type histogram;
//! - the features with the most vertices (geometry hotspots);
//! - the largest property-table value entries (long strings / high cardinality);
//! - a one-line verdict on whether the tile is geometry- or attribute-dominated.
//!
//! All byte figures are **uncompressed** MVT content — that is what the user can
//! actually shrink (compression ratio is downstream). The stored/compressed size
//! is reported alongside for reference.

use anyhow::{Result, anyhow, ensure};
use versatiles_container::{SharedTileSource, TilesRuntime};
use versatiles_core::{TileCoord, TileType, utils::PrettyPrint};
use versatiles_geometry::geo::GeoValue;
use versatiles_geometry::vector_tile::{GeoValuePBF, GeomType, VectorTile};

/// How many entries to show in the "top features" and "top values" tables.
const TOP_N: usize = 10;

/// Upper bound on how many tiles a single invocation may analyze when explicit
/// `--tile` ranges are given. Keeps a wide bbox from flooding the terminal.
const MAX_SELECTED_TILES: usize = 64;

/// A parsed `z/x/y` selector: zoom plus inclusive `(lo, hi)` x and y ranges.
type TileSelector = (u8, (u32, u32), (u32, u32));

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
/// Analyze why one or more vector tiles are large, breaking each tile down by
/// layer into geometry, tag, and property-table bytes.
///
/// Pass one or more tiles as `z/x/y` via `--tile` (repeatable). Each axis may be
/// a range, e.g. `--tile 14/8800-8803/5371-5374` selects a 4×4 block. Without
/// any `--tile`, the container is scanned and the `--count` biggest tiles are
/// analyzed automatically.
pub struct AnalyzeTile {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// Tile(s) to analyze in `z/x/y` form; each of x and y may be a `lo-hi`
	/// range (e.g. `14/8800-8803/5371-5374`). Repeat to analyze several.
	#[arg(long, short = 't', value_name = "Z/X/Y", verbatim_doc_comment)]
	tile: Vec<String>,

	/// In scan mode (no `--tile`), how many of the biggest tiles to analyze.
	#[arg(long, default_value_t = 3)]
	count: usize,
}

pub async fn run(args: &AnalyzeTile, runtime: &TilesRuntime) -> Result<()> {
	let reader = runtime.reader_from_str(&args.input).await?;

	ensure!(
		reader.metadata().tile_format().to_type() == TileType::Vector,
		"analyze-tile only supports vector tile sources (format: {:?})",
		reader.metadata().tile_format()
	);

	let coords = resolve_coords(args, &reader, runtime).await?;
	ensure!(!coords.is_empty(), "no tiles to analyze");

	let mut print = PrettyPrint::new();
	for coord in coords {
		analyze_one(&reader, coord, &mut print).await?;
	}
	Ok(())
}

/// Determine which tiles to analyze: the explicit `--tile` selectors if any were
/// given, otherwise the `--count` biggest tiles found by scanning the container.
async fn resolve_coords(args: &AnalyzeTile, reader: &SharedTileSource, runtime: &TilesRuntime) -> Result<Vec<TileCoord>> {
	if args.tile.is_empty() {
		ensure!(args.count > 0, "--count must be at least 1");
		return biggest_tile_coords(reader, args.count, runtime).await;
	}

	let mut coords = Vec::new();
	for selector in &args.tile {
		let (z, (x0, x1), (y0, y1)) = parse_tile_selector(selector)?;
		let count = (u64::from(x1 - x0 + 1)) * (u64::from(y1 - y0 + 1));
		ensure!(
			coords.len() as u64 + count <= MAX_SELECTED_TILES as u64,
			"selection covers more than {MAX_SELECTED_TILES} tiles — narrow the x/y ranges or analyze fewer tiles at once"
		);
		for x in x0..=x1 {
			for y in y0..=y1 {
				coords.push(TileCoord::new(z, x, y)?);
			}
		}
	}
	Ok(coords)
}

/// Parse a `z/x/y` tile selector, where each of x and y may be a single value or
/// an inclusive `lo-hi` range. Returns the zoom plus the (inclusive) x and y
/// ranges.
fn parse_tile_selector(selector: &str) -> Result<TileSelector> {
	let parts: Vec<&str> = selector.split('/').collect();
	ensure!(
		parts.len() == 3,
		"tile selector must be in z/x/y form (got {selector:?})"
	);
	let z: u8 = parts[0]
		.parse()
		.map_err(|_| anyhow!("invalid zoom {:?} in selector {selector:?}", parts[0]))?;
	let x = parse_axis_range(parts[1]).map_err(|e| e.context(format!("parsing x of {selector:?}")))?;
	let y = parse_axis_range(parts[2]).map_err(|e| e.context(format!("parsing y of {selector:?}")))?;
	Ok((z, x, y))
}

/// Parse a single axis component: either `n` (→ `(n, n)`) or `lo-hi` (→ inclusive
/// `(lo, hi)`, requiring `lo <= hi`).
fn parse_axis_range(part: &str) -> Result<(u32, u32)> {
	if let Some((lo, hi)) = part.split_once('-') {
		let lo: u32 = lo.parse().map_err(|_| anyhow!("invalid range start {lo:?}"))?;
		let hi: u32 = hi.parse().map_err(|_| anyhow!("invalid range end {hi:?}"))?;
		ensure!(lo <= hi, "range start {lo} must not exceed end {hi}");
		Ok((lo, hi))
	} else {
		let value: u32 = part.parse().map_err(|_| anyhow!("invalid coordinate {part:?}"))?;
		Ok((value, value))
	}
}

/// Scan the whole pyramid by stored tile size and return the coordinates of the
/// `count` largest tiles, biggest first.
async fn biggest_tile_coords(
	reader: &SharedTileSource,
	count: usize,
	runtime: &TilesRuntime,
) -> Result<Vec<TileCoord>> {
	let pyramid = reader.tile_pyramid().await?;
	let progress = runtime.create_progress("scanning for biggest tiles", pyramid.count_tiles());

	// (size, coord), kept sorted biggest-first, truncated to `count`.
	let mut biggest: Vec<(u32, TileCoord)> = Vec::with_capacity(count + 1);
	for bbox in pyramid.to_iter_bboxes().filter(|b| !b.is_empty()) {
		let mut stream = reader.tile_size_stream(bbox).await?;
		while let Some((coord, size)) = stream.next().await {
			progress.inc(1);
			if biggest.len() == count && size <= biggest.last().expect("non-empty").0 {
				continue;
			}
			let pos = biggest.partition_point(|e| e.0 >= size);
			biggest.insert(pos, (size, coord));
			biggest.truncate(count);
		}
	}
	progress.finish();

	Ok(biggest.into_iter().map(|(_, coord)| coord).collect())
}

/// Accumulated byte breakdown for a single layer within the tile.
struct LayerStats {
	name: String,
	feature_count: usize,
	vertex_count: usize,
	/// Sum of `feature.geom_data.len()` — the geometry command streams.
	geometry_bytes: usize,
	/// Sum of the packed `tag_ids` varint lengths — per-feature property refs.
	tag_bytes: usize,
	/// Sum of key-string lengths in the property table.
	key_bytes: usize,
	/// Sum of encoded value-message lengths in the property table.
	value_bytes: usize,
	/// Exact encoded layer size (`layer.to_blob().len()`), used to derive the
	/// residual "other" framing/overhead so the columns sum to the total.
	encoded_bytes: usize,
}

impl LayerStats {
	/// Property-table bytes: keys + values (not the per-feature tag refs).
	fn property_bytes(&self) -> usize {
		self.key_bytes + self.value_bytes
	}

	/// Residual bytes not attributed to geometry/tags/properties: feature ids,
	/// geom-type fields, the layer name, and all the PBF key/length framing.
	fn other_bytes(&self) -> usize {
		self
			.encoded_bytes
			.saturating_sub(self.geometry_bytes + self.tag_bytes + self.property_bytes())
	}
}

/// One geometry hotspot: a feature with a notably high vertex count.
struct FeatureEntry {
	layer: String,
	geom_type: GeomType,
	vertices: usize,
	geom_bytes: usize,
	id: Option<u64>,
}

/// One property-table value entry, for spotting oversized / high-cardinality
/// attribute values (long strings, embedded JSON, ...).
struct ValueEntry {
	layer: String,
	bytes: usize,
	type_name: &'static str,
	display: String,
}

/// The full analysis of one decoded tile.
struct TileAnalysis {
	layers: Vec<LayerStats>,
	top_features: Vec<FeatureEntry>,
	top_values: Vec<ValueEntry>,
	uncompressed_bytes: usize,
}

impl TileAnalysis {
	fn total_geometry(&self) -> usize {
		self.layers.iter().map(|l| l.geometry_bytes).sum()
	}
	fn total_attributes(&self) -> usize {
		self.layers.iter().map(|l| l.tag_bytes + l.property_bytes()).sum()
	}
	fn feature_count(&self) -> usize {
		self.layers.iter().map(|l| l.feature_count).sum()
	}
	fn vertex_count(&self) -> usize {
		self.layers.iter().map(|l| l.vertex_count).sum()
	}
}

/// Decode and analyze a single tile, writing a report section into `print`.
async fn analyze_one(reader: &SharedTileSource, coord: TileCoord, print: &mut PrettyPrint) -> Result<()> {
	let cat = print
		.category(&format!("tile {}/{}/{}", coord.level, coord.x, coord.y))
		.await;

	let Some(mut tile) = reader.tile(&coord).await? else {
		cat.add_warning("tile not found in container").await;
		return Ok(());
	};

	// Stored/compressed size for reference, then decode for the breakdown.
	let compression = *reader.metadata().tile_compression();
	let compressed_bytes = usize::try_from(tile.as_blob(&compression)?.len())?;
	let vt = tile.into_vector()?;

	let analysis = analyze_vector_tile(&vt)?;

	cat.add_key_value("compressed size", &fmt_bytes(compressed_bytes)).await;
	cat.add_key_value("uncompressed size", &fmt_bytes(analysis.uncompressed_bytes))
		.await;
	cat.add_key_value("layers", &analysis.layers.len()).await;
	cat.add_key_value("features", &analysis.feature_count()).await;
	cat.add_key_value("vertices", &analysis.vertex_count()).await;

	add_layer_table(&cat, &analysis).await;
	add_geom_type_table(&cat, &vt).await;
	add_top_features_table(&cat, &analysis).await;
	add_top_values_table(&cat, &analysis).await;

	cat.new_line().await;
	cat.add_key_value("verdict", &verdict(&analysis)).await;

	Ok(())
}

/// Walk the decoded tile and accumulate per-layer stats plus the cross-layer
/// top-feature and top-value lists.
fn analyze_vector_tile(vt: &VectorTile) -> Result<TileAnalysis> {
	let mut layers: Vec<LayerStats> = Vec::with_capacity(vt.layers.len());
	let mut top_features: Vec<FeatureEntry> = Vec::new();
	let mut top_values: Vec<ValueEntry> = Vec::new();

	for layer in &vt.layers {
		let mut stats = LayerStats {
			name: layer.name.clone(),
			feature_count: layer.features.len(),
			vertex_count: 0,
			geometry_bytes: 0,
			tag_bytes: 0,
			key_bytes: 0,
			value_bytes: 0,
			encoded_bytes: usize::try_from(layer.to_blob()?.len())?,
		};

		for feature in &layer.features {
			let geom_bytes = usize::try_from(feature.geom_data.len())?;
			let vertices = feature.count_geometry_points();
			let tag_bytes: usize = feature.tag_ids.iter().map(|id| varint_len(u64::from(*id))).sum();

			stats.geometry_bytes += geom_bytes;
			stats.vertex_count += vertices;
			stats.tag_bytes += tag_bytes;

			top_features.push(FeatureEntry {
				layer: layer.name.clone(),
				geom_type: feature.geom_type,
				vertices,
				geom_bytes,
				id: feature.id,
			});
		}

		for key in layer.property_manager.iter_key() {
			stats.key_bytes += key.len();
		}
		for value in layer.property_manager.iter_val() {
			let bytes = GeoValuePBF::to_blob(value).map_or(0, |b| usize::try_from(b.len()).unwrap_or(0));
			stats.value_bytes += bytes;
			top_values.push(ValueEntry {
				layer: layer.name.clone(),
				bytes,
				type_name: value_type_name(value),
				display: render_value(value),
			});
		}

		layers.push(stats);
	}

	layers.sort_by_key(|l| std::cmp::Reverse(l.encoded_bytes));
	top_features.sort_by_key(|f| std::cmp::Reverse(f.vertices));
	top_features.truncate(TOP_N);
	top_values.sort_by_key(|v| std::cmp::Reverse(v.bytes));
	top_values.truncate(TOP_N);

	let uncompressed_bytes = usize::try_from(vt.to_blob()?.len())?;

	Ok(TileAnalysis {
		layers,
		top_features,
		top_values,
		uncompressed_bytes,
	})
}

async fn add_layer_table(print: &PrettyPrint, analysis: &TileAnalysis) {
	print.new_line().await;
	let total = analysis.uncompressed_bytes.max(1);
	let rows: Vec<Vec<String>> = analysis
		.layers
		.iter()
		.map(|l| {
			vec![
				l.name.clone(),
				fmt_int(l.feature_count),
				fmt_int(l.vertex_count),
				fmt_bytes(l.geometry_bytes),
				fmt_bytes(l.tag_bytes),
				fmt_bytes(l.property_bytes()),
				fmt_bytes(l.other_bytes()),
				fmt_bytes(l.encoded_bytes),
				format!("{}%", l.encoded_bytes * 100 / total),
			]
		})
		.collect();
	print
		.add_table(
			"size by layer",
			&[
				"layer", "feats", "verts", "geometry", "tags", "props", "other", "total", "%",
			],
			&rows,
		)
		.await;
}

async fn add_geom_type_table(print: &PrettyPrint, vt: &VectorTile) {
	print.new_line().await;
	// (feature count, vertex count, geometry bytes) keyed by geometry type.
	let mut buckets: std::collections::BTreeMap<&'static str, (usize, usize, usize)> = std::collections::BTreeMap::new();
	for layer in &vt.layers {
		for feature in &layer.features {
			let entry = buckets.entry(geom_type_name(feature.geom_type)).or_default();
			entry.0 += 1;
			entry.1 += feature.count_geometry_points();
			entry.2 += usize::try_from(feature.geom_data.len()).unwrap_or(0);
		}
	}
	let rows: Vec<Vec<String>> = buckets
		.into_iter()
		.map(|(name, (feats, verts, bytes))| vec![name.to_string(), fmt_int(feats), fmt_int(verts), fmt_bytes(bytes)])
		.collect();
	print
		.add_table("geometry types", &["type", "feats", "verts", "geometry"], &rows)
		.await;
}

async fn add_top_features_table(print: &PrettyPrint, analysis: &TileAnalysis) {
	let rows: Vec<Vec<String>> = analysis
		.top_features
		.iter()
		.filter(|f| f.vertices > 0)
		.map(|f| {
			vec![
				f.layer.clone(),
				geom_type_name(f.geom_type).to_string(),
				fmt_int(f.vertices),
				fmt_bytes(f.geom_bytes),
				f.id.map_or_else(|| "-".to_string(), |id| id.to_string()),
			]
		})
		.collect();
	if !rows.is_empty() {
		print.new_line().await;
		print
			.add_table(
				&format!("top {} features by vertices", rows.len()),
				&["layer", "type", "verts", "geometry", "id"],
				&rows,
			)
			.await;
	}
}

async fn add_top_values_table(print: &PrettyPrint, analysis: &TileAnalysis) {
	let rows: Vec<Vec<String>> = analysis
		.top_values
		.iter()
		.filter(|v| v.bytes > 0)
		.map(|v| {
			vec![
				v.layer.clone(),
				fmt_bytes(v.bytes),
				v.type_name.to_string(),
				v.display.clone(),
			]
		})
		.collect();
	if !rows.is_empty() {
		print.new_line().await;
		print
			.add_table(
				&format!("top {} property values by size", rows.len()),
				&["layer", "bytes", "type", "value"],
				&rows,
			)
			.await;
	}
}

/// One-line diagnosis of where the bytes are going.
fn verdict(analysis: &TileAnalysis) -> String {
	let geom = analysis.total_geometry();
	let attr = analysis.total_attributes();
	let total = (geom + attr).max(1);
	let geom_pct = geom * 100 / total;
	let attr_pct = attr * 100 / total;

	if geom >= attr * 2 {
		format!(
			"geometry-dominated ({geom_pct}% geometry) — simplify more aggressively or raise max_zoom so detail spreads over more tiles"
		)
	} else if attr >= geom * 2 {
		format!(
			"attribute-dominated ({attr_pct}% attributes) — prune properties, shorten long string values, or lower attribute cardinality"
		)
	} else {
		format!("mixed ({geom_pct}% geometry / {attr_pct}% attributes) — both geometry detail and attributes contribute")
	}
}

/// Byte length of `value` as an unsigned LEB128 varint (matches the MVT/PBF
/// packed `tag_ids` encoding).
fn varint_len(mut value: u64) -> usize {
	let mut len = 1;
	while value >= 0x80 {
		value >>= 7;
		len += 1;
	}
	len
}

fn geom_type_name(geom_type: GeomType) -> &'static str {
	match geom_type {
		GeomType::Unknown => "Unknown",
		GeomType::MultiPoint => "Point",
		GeomType::MultiLineString => "LineString",
		GeomType::MultiPolygon => "Polygon",
	}
}

fn value_type_name(value: &GeoValue) -> &'static str {
	match value {
		GeoValue::Bool(_) => "Bool",
		GeoValue::Double(_) | GeoValue::Float(_) | GeoValue::Int(_) | GeoValue::UInt(_) => "Number",
		GeoValue::String(_) => "String",
		GeoValue::Null => "Null",
	}
}

/// Render a property value for display, truncating long strings on a char
/// boundary so the table stays readable.
fn render_value(value: &GeoValue) -> String {
	let s = format!("{value:?}");
	const MAX: usize = 60;
	if s.chars().count() > MAX {
		let truncated: String = s.chars().take(MAX).collect();
		format!("{truncated}…")
	} else {
		s
	}
}

/// Format an integer with `_` thousands separators (e.g. `1_234_567`).
fn fmt_int(v: usize) -> String {
	let s = v.to_string();
	let mut out = String::with_capacity(s.len() + s.len() / 3);
	for (i, c) in s.chars().enumerate() {
		if i > 0 && (s.len() - i).is_multiple_of(3) {
			out.push('_');
		}
		out.push(c);
	}
	out
}

/// Same grouping as [`fmt_int`]; named separately to keep call sites
/// self-documenting about whether a column is a byte count.
fn fmt_bytes(v: usize) -> String {
	fmt_int(v)
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles::runtime::create_test_runtime;
	use versatiles_geometry::{
		geo::{GeoFeature, GeoProperties},
		vector_tile::{VectorTile, VectorTileLayer},
	};

	use geo_types::{Geometry, LineString, Point};

	fn line_feature(coords: Vec<[f64; 2]>, props: Vec<(&str, GeoValue)>) -> GeoFeature {
		GeoFeature {
			id: Some(GeoValue::from(1_u64)),
			geometry: Geometry::LineString(LineString::from(coords)),
			properties: GeoProperties::from(props),
		}
	}

	fn point_feature(x: f64, y: f64, props: Vec<(&str, GeoValue)>) -> GeoFeature {
		GeoFeature {
			id: Some(GeoValue::from(2_u64)),
			geometry: Geometry::Point(Point::new(x, y)),
			properties: GeoProperties::from(props),
		}
	}

	#[test]
	fn parse_axis_range_handles_single_and_range() {
		assert_eq!(parse_axis_range("5").unwrap(), (5, 5));
		assert_eq!(parse_axis_range("8800-8803").unwrap(), (8800, 8803));
		// Reversed ranges and garbage are rejected.
		assert!(parse_axis_range("8803-8800").is_err());
		assert!(parse_axis_range("abc").is_err());
		assert!(parse_axis_range("1-").is_err());
	}

	#[test]
	fn parse_tile_selector_parses_point_and_box() {
		assert_eq!(parse_tile_selector("14/8802/5374").unwrap(), (14, (8802, 8802), (5374, 5374)));
		assert_eq!(
			parse_tile_selector("14/8800-8803/5371-5374").unwrap(),
			(14, (8800, 8803), (5371, 5374))
		);
		// Mixed: single x, ranged y.
		assert_eq!(parse_tile_selector("14/8802/5371-5374").unwrap(), (14, (8802, 8802), (5371, 5374)));
		// Wrong shape.
		assert!(parse_tile_selector("14/8802").is_err());
		assert!(parse_tile_selector("14/8802/5374/extra").is_err());
		assert!(parse_tile_selector("z/8802/5374").is_err());
	}

	#[tokio::test]
	async fn resolve_coords_expands_ranges_and_enforces_cap() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;

		// A 4×4 block expands to 16 coords.
		let args = AnalyzeTile {
			input: "../testdata/berlin.mbtiles".into(),
			tile: vec!["14/8800-8803/5371-5374".into()],
			count: 3,
		};
		let coords = resolve_coords(&args, &reader, &runtime).await?;
		assert_eq!(coords.len(), 16);
		assert!(coords.iter().all(|c| c.level == 14));

		// Multiple selectors accumulate.
		let args = AnalyzeTile {
			input: "../testdata/berlin.mbtiles".into(),
			tile: vec!["14/8802/5374".into(), "5/9/11".into()],
			count: 3,
		};
		let coords = resolve_coords(&args, &reader, &runtime).await?;
		assert_eq!(coords.len(), 2);

		// Over-cap selection is rejected.
		let args = AnalyzeTile {
			input: "../testdata/berlin.mbtiles".into(),
			tile: vec!["8/0-15/0-15".into()],
			count: 3,
		};
		assert!(resolve_coords(&args, &reader, &runtime).await.is_err());
		Ok(())
	}

	#[test]
	fn varint_len_matches_leb128_boundaries() {
		assert_eq!(varint_len(0), 1);
		assert_eq!(varint_len(127), 1);
		assert_eq!(varint_len(128), 2);
		assert_eq!(varint_len(16_383), 2);
		assert_eq!(varint_len(16_384), 3);
	}

	#[test]
	fn fmt_int_groups_thousands() {
		assert_eq!(fmt_int(0), "0");
		assert_eq!(fmt_int(999), "999");
		assert_eq!(fmt_int(1_000), "1_000");
		assert_eq!(fmt_int(1_234_567), "1_234_567");
	}

	#[test]
	fn render_value_truncates_long_strings_on_char_boundary() {
		let long = "ä".repeat(100);
		let rendered = render_value(&GeoValue::from(long));
		assert!(rendered.ends_with('…'));
		// 60 chars taken + the ellipsis. The leading `String("` from Debug counts
		// toward the 60, so just assert it did not panic and is bounded.
		assert!(rendered.chars().count() <= 61);
	}

	#[test]
	fn analyze_vector_tile_counts_geometry_and_attributes() -> Result<()> {
		// A line with many vertices (geometry-heavy) plus a point carrying a
		// chunky string (attribute-heavy) across two layers.
		let big_line = line_feature((0..50).map(|i| [f64::from(i), f64::from(i)]).collect(), vec![]);
		let labelled = point_feature(
			10.0,
			10.0,
			vec![
				("name", GeoValue::from("A".repeat(200))),
				("rank", GeoValue::from(5_u64)),
			],
		);

		let roads = VectorTileLayer::from_features("roads".to_string(), vec![big_line], 4096, 1)?;
		let labels = VectorTileLayer::from_features("labels".to_string(), vec![labelled], 4096, 1)?;
		let vt = VectorTile::new(vec![roads, labels]);

		let analysis = analyze_vector_tile(&vt)?;

		assert_eq!(analysis.layers.len(), 2);
		assert_eq!(analysis.feature_count(), 2);
		assert!(analysis.vertex_count() >= 50, "line should contribute ~50 vertices");
		assert!(analysis.total_geometry() > 0);
		assert!(analysis.total_attributes() > 0);

		// The roads layer geometry should be the biggest single feature by verts.
		assert_eq!(analysis.top_features[0].layer, "roads");
		assert!(analysis.top_features[0].vertices >= 50);

		// The 200-char string should be the largest property value.
		assert_eq!(analysis.top_values[0].layer, "labels");
		assert!(analysis.top_values[0].bytes >= 200);
		assert_eq!(analysis.top_values[0].type_name, "String");

		// Per-layer columns must not exceed the layer's own encoded size.
		for l in &analysis.layers {
			assert!(l.geometry_bytes + l.tag_bytes + l.property_bytes() <= l.encoded_bytes);
		}
		Ok(())
	}

	#[test]
	fn verdict_classifies_geometry_vs_attribute_dominated() {
		let geom_heavy = TileAnalysis {
			layers: vec![LayerStats {
				name: "x".into(),
				feature_count: 1,
				vertex_count: 1000,
				geometry_bytes: 10_000,
				tag_bytes: 100,
				key_bytes: 50,
				value_bytes: 50,
				encoded_bytes: 10_300,
			}],
			top_features: vec![],
			top_values: vec![],
			uncompressed_bytes: 10_300,
		};
		assert!(verdict(&geom_heavy).contains("geometry-dominated"));

		let attr_heavy = TileAnalysis {
			layers: vec![LayerStats {
				name: "x".into(),
				feature_count: 1,
				vertex_count: 1,
				geometry_bytes: 100,
				tag_bytes: 5_000,
				key_bytes: 2_500,
				value_bytes: 2_500,
				encoded_bytes: 10_100,
			}],
			top_features: vec![],
			top_values: vec![],
			uncompressed_bytes: 10_100,
		};
		assert!(verdict(&attr_heavy).contains("attribute-dominated"));
	}

	#[tokio::test]
	async fn biggest_tile_coords_returns_requested_count() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let coords = biggest_tile_coords(&reader, 3, &runtime).await?;
		assert_eq!(coords.len(), 3);
		Ok(())
	}

	#[tokio::test]
	async fn analyze_one_against_mbtiles_produces_report() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let coords = biggest_tile_coords(&reader, 1, &runtime).await?;

		let mut print = PrettyPrint::new();
		analyze_one(&reader, coords[0], &mut print).await?;
		let out = print.stringify().await;

		assert!(out.contains("size by layer"), "got: {out}");
		assert!(out.contains("geometry types"), "got: {out}");
		assert!(out.contains("verdict"), "got: {out}");
		assert!(out.contains("uncompressed size"), "got: {out}");
		Ok(())
	}
}
