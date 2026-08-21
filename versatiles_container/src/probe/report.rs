//! Container analysis as a value.
//!
//! Everything `versatiles probe` works out about a container — format, compression,
//! the real tile pyramid, tile sizes, and the MVT validation plus per-layer byte
//! breakdown of a deep scan — is computed here and returned, rather than rendered
//! as it is computed. The CLI's `PrettyPrint` output is then one consumer of this
//! report rather than the only way to reach the answers.
//!
//! # What is not here
//!
//! [`ProbeDepth::Container`]'s format-specific details come from
//! [`TileSource::probe_container`], which writes to a `PrettyPrint` and is
//! implemented per container. Turning that into data means changing every
//! container implementation, so it stays a rendering hook and the CLI calls it
//! directly.

use std::collections::HashMap;

use anyhow::Result;
use versatiles_core::{GeoBBox, ProbeDepth, TileBBox, TileCompression, TileFormat, TileJSON, TileSize, TileType};
use versatiles_geometry::vector_tile::{
	DegenerateReason, GeomType, IssueKind, LayerStats, ValidationIssue, layer_stats, validate_tile,
};

use super::sampling::build_scan_plan;
use crate::{Tile, TileSource, TilesRuntime};

/// How many issue examples a deep scan keeps.
pub const VALIDATION_SAMPLE_LIMIT: usize = 10;

/// How many of the biggest tiles a size scan keeps.
pub const BIGGEST_TILES_LIMIT: usize = 10;

/// Everything a probe works out about a container.
///
/// Which fields are populated depends on the [`ProbeDepth`] the report was built
/// at: `tile_sizes` needs [`ProbeDepth::TileSizes`] or deeper, `contents` needs
/// [`ProbeDepth::TileContents`].
#[derive(Clone, Debug)]
pub struct ProbeReport {
	/// Human-readable description of the source, e.g. `container 'mbtiles' ('…')`.
	pub source_type: String,
	/// The format tiles are stored in.
	pub tile_format: TileFormat,
	/// The compression tiles are stored with.
	pub tile_compression: TileCompression,
	/// The source's TileJSON, as advertised.
	pub tilejson: TileJSON,
	/// One entry per non-empty zoom level, ascending. Derived from the actual
	/// pyramid rather than from the TileJSON's declared zoom range.
	pub pyramid: Vec<PyramidLevel>,
	/// Pixel size measured by decoding one tile, set only when the TileJSON
	/// declares no `tile_size` and one could be established.
	///
	/// Kept apart from `tilejson` on purpose: what the source *says* and what its
	/// tiles *are* are different claims, and a probe that quietly merged them
	/// would hide exactly the gap this reports (issue #247). Note that
	/// `tile_sizes` below is unrelated — those are byte sizes.
	pub measured_tile_size: Option<TileSize>,
	/// Populated at [`ProbeDepth::TileSizes`] and deeper.
	pub tile_sizes: Option<TileSizeReport>,
	/// Populated at [`ProbeDepth::TileContents`].
	pub contents: Option<ContentsReport>,
}

impl ProbeReport {
	/// The real zoom range, from the non-empty levels of the pyramid.
	///
	/// `None` for a source with no tiles at all. This is what the container
	/// actually holds, which can be narrower than the TileJSON claims.
	#[must_use]
	pub fn zoom(&self) -> Option<std::ops::RangeInclusive<u8>> {
		let first = self.pyramid.first()?;
		let last = self.pyramid.last()?;
		Some(first.level..=last.level)
	}

	/// Geographic bounds as `[west, south, east, north]`, from the TileJSON.
	#[must_use]
	pub fn bbox(&self) -> Option<[f64; 4]> {
		self.tilejson.bounds.as_ref().map(GeoBBox::as_array)
	}
}

/// One non-empty zoom level of the source's tile pyramid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PyramidLevel {
	pub level: u8,
	pub x_min: u32,
	pub x_max: u32,
	pub y_min: u32,
	pub y_max: u32,
	/// Tiles actually present at this level.
	pub tiles: u64,
	/// Percentage of this level's bounding box that holds a tile.
	pub coverage_percent: u64,
}

/// Result of scanning every tile's size.
#[derive(Clone, Debug, Default)]
pub struct TileSizeReport {
	pub tile_count: u64,
	/// Total bytes across all tiles, as stored.
	pub size_sum: u64,
	/// The biggest tiles, largest first — at most [`BIGGEST_TILES_LIMIT`].
	pub biggest_tiles: Vec<TileSizeEntry>,
	/// Per zoom level, ascending.
	pub per_level: Vec<LevelSizeStats>,
}

impl TileSizeReport {
	/// Mean tile size in bytes, or `0` for a source with no tiles.
	#[must_use]
	pub fn average_size(&self) -> u64 {
		self.size_sum.checked_div(self.tile_count).unwrap_or(0)
	}
}

/// One tile and its stored size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSizeEntry {
	pub z: u8,
	pub x: u32,
	pub y: u32,
	pub size: u64,
}

/// Tile-size totals for one zoom level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelSizeStats {
	pub level: u8,
	pub count: u64,
	pub size_sum: u64,
}

impl LevelSizeStats {
	#[must_use]
	pub fn average_size(&self) -> u64 {
		self.size_sum.checked_div(self.count).unwrap_or(0)
	}
}

/// Result of reading every tile's contents.
#[derive(Clone, Debug)]
pub enum ContentsReport {
	/// The source is not vector. There are no content-level diagnostics for
	/// raster tiles yet.
	Unsupported,
	/// MVT validation and per-layer byte breakdown.
	Vector(VectorContentsReport),
}

/// MVT validation findings and byte breakdown across a whole container.
#[derive(Clone, Debug, Default)]
pub struct VectorContentsReport {
	/// The fraction of the deepest zoom level that was read, when sampling.
	/// `None` means every tile was read.
	pub sample_fraction: Option<f64>,
	/// Tiles actually read, including ones that failed to decode.
	pub tiles_scanned: u64,
	pub issues: ValidationCounters,
	/// Up to [`VALIDATION_SAMPLE_LIMIT`] concrete examples, in encounter order.
	pub samples: Vec<IssueSample>,
	/// Byte breakdown per (zoom, layer), ordered by zoom ascending then bytes
	/// descending — the order a report renders in.
	pub layer_sizes: Vec<LayerSizeEntry>,
}

impl VectorContentsReport {
	/// Byte breakdown per layer across every zoom, biggest first.
	///
	/// The roll-up of [`layer_sizes`](Self::layer_sizes); computed here so a
	/// consumer does not have to re-derive which layer dominates the container.
	#[must_use]
	pub fn layer_totals(&self) -> Vec<(String, LayerStats)> {
		let mut per_layer: HashMap<&str, LayerStats> = HashMap::new();
		for entry in &self.layer_sizes {
			per_layer.entry(&entry.layer).or_default().add(&entry.stats);
		}
		let mut totals: Vec<(String, LayerStats)> = per_layer
			.into_iter()
			.map(|(name, stats)| (name.to_string(), stats))
			.collect();
		// Ties broken by name so the order is stable across runs — a HashMap
		// would otherwise reorder equal-sized layers between invocations.
		totals.sort_by(|a, b| b.1.encoded_bytes.cmp(&a.1.encoded_bytes).then(a.0.cmp(&b.0)));
		totals
	}

	/// Total uncompressed bytes across every layer and zoom.
	#[must_use]
	pub fn total_bytes(&self) -> usize {
		self.layer_sizes.iter().map(|e| e.stats.encoded_bytes).sum()
	}

	/// Whether any layer carries feature ids — decides whether an `ids` column
	/// is worth showing.
	#[must_use]
	pub fn has_feature_ids(&self) -> bool {
		self.layer_sizes.iter().any(|e| e.stats.id_bytes > 0)
	}
}

/// Byte breakdown of one layer at one zoom level, summed over every tile.
#[derive(Clone, Debug)]
pub struct LayerSizeEntry {
	pub zoom: u8,
	pub layer: String,
	/// Tiles at this zoom that contained this layer.
	pub tiles: u64,
	pub stats: LayerStats,
}

/// One concrete spec violation, kept as an example.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueSample {
	pub z: u8,
	pub x: u32,
	pub y: u32,
	pub layer: String,
	pub feature_index: Option<usize>,
	pub kind: String,
}

/// Counts of each kind of MVT spec violation found across a container.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValidationCounters {
	pub missing_extent: u64,
	pub missing_version: u64,
	pub duplicate_layer_name: u64,
	pub orphan_inner: u64,
	pub degenerate_too_few: u64,
	pub degenerate_sub_pixel: u64,
	pub degenerate_collinear: u64,
	pub unknown_geom: u64,
	pub empty_geom_point: u64,
	pub empty_geom_line: u64,
	pub empty_geom_polygon: u64,
	pub malformed_stream: u64,
	/// Tiles that could not be decoded as vector tiles at all. Counted, but not
	/// validated, so excluded from [`total_issues`](Self::total_issues).
	pub decode_failures: u64,
	pub tiles_with_issues: u64,
}

impl ValidationCounters {
	/// Total spec violations, excluding decode failures.
	#[must_use]
	pub fn total_issues(&self) -> u64 {
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

	/// Issues that `vector_repair` fixes on its own.
	#[must_use]
	pub fn fixable_automatically(&self) -> u64 {
		self.missing_extent
			+ self.missing_version
			+ self.duplicate_layer_name
			+ self.orphan_inner
			+ self.degenerate_too_few
			+ self.degenerate_sub_pixel
			+ self.degenerate_collinear
	}

	/// Issues that need `vector_repair drop_offenders=true`, because the
	/// offending feature cannot be salvaged.
	#[must_use]
	pub fn needs_drop_offenders(&self) -> u64 {
		self.unknown_geom + self.empty_geom_point + self.empty_geom_line + self.empty_geom_polygon + self.malformed_stream
	}

	/// Counts, in the order a summary lists them. Kinds with no occurrences are
	/// included; a caller filters if it wants only what was found.
	#[must_use]
	pub fn by_kind(&self) -> Vec<(&'static str, u64)> {
		vec![
			("MissingExtent", self.missing_extent),
			("MissingVersion", self.missing_version),
			("DuplicateLayerName", self.duplicate_layer_name),
			("OrphanInnerRing", self.orphan_inner),
			("DegenerateRing(TooFewVertices)", self.degenerate_too_few),
			("DegenerateRing(SubPixel)", self.degenerate_sub_pixel),
			("DegenerateRing(Collinear)", self.degenerate_collinear),
			("UnknownGeometryType", self.unknown_geom),
			("EmptyGeometryForType(MultiPoint)", self.empty_geom_point),
			("EmptyGeometryForType(MultiLineString)", self.empty_geom_line),
			("EmptyGeometryForType(MultiPolygon)", self.empty_geom_polygon),
			("MalformedCommandStream", self.malformed_stream),
		]
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

/// Names an [`IssueKind`] for display, without its payload.
#[must_use]
pub fn describe_issue_kind(kind: &IssueKind) -> String {
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

/// Analyses `source` to the requested depth and returns what it found.
///
/// The work done at each depth is the same work `versatiles probe` has always
/// done; only the result differs. `sample` is the fraction of the deepest zoom
/// level to read during a [`ProbeDepth::TileContents`] scan, from
/// [`super::sampling::parse_sample`]; `None` reads every tile.
///
/// # Errors
/// Returns an error if the source's pyramid cannot be read, or if a tile stream
/// fails mid-scan.
pub async fn probe_report(
	source: &dyn TileSource,
	depth: ProbeDepth,
	runtime: &TilesRuntime,
	sample: Option<f64>,
) -> Result<ProbeReport> {
	use ProbeDepth::{TileContents, TileSizes};

	let metadata = source.metadata();
	let tile_pyramid = source.tile_pyramid().await?;

	let pyramid = tile_pyramid
		.iter()
		.filter(|level| !level.is_empty())
		.map(|level| {
			let bbox = level.to_bbox();
			let tiles = level.count_tiles();
			PyramidLevel {
				level: level.level(),
				x_min: bbox.x_min().unwrap_or(0),
				x_max: bbox.x_max().unwrap_or(0),
				y_min: bbox.y_min().unwrap_or(0),
				y_max: bbox.y_max().unwrap_or(0),
				tiles,
				coverage_percent: coverage_percent(tiles, bbox.count_tiles()),
			}
		})
		.collect();

	let tilejson = source.tilejson().clone();
	let measured_tile_size = if tilejson.tile_size.is_none() {
		source.measure_tile_size().await?
	} else {
		None
	};

	let mut report = ProbeReport {
		source_type: source.source_type().to_string(),
		tile_format: *metadata.tile_format(),
		tile_compression: *metadata.tile_compression(),
		tilejson,
		pyramid,
		measured_tile_size,
		tile_sizes: None,
		contents: None,
	};

	if matches!(depth, TileSizes | TileContents) {
		report.tile_sizes = Some(scan_tile_sizes(source, runtime).await?);
	}

	if matches!(depth, TileContents) {
		report.contents = Some(if metadata.tile_format().to_type() == TileType::Vector {
			ContentsReport::Vector(scan_vector_contents(source, runtime, sample).await?)
		} else {
			ContentsReport::Unsupported
		});
	}

	Ok(report)
}

/// What share of a level's bounding box holds a tile, as a whole percentage.
///
/// `u128` because the obvious `tiles * 100 / total` overflows `u64`: a full
/// level 29 holds 2^58 tiles, and multiplying that by 100 passes `u64::MAX`.
/// That is not a hypothetical depth — a source can advertise levels 0–30, and
/// `from_debug` does. With overflow checks on it panicked; without them, in the
/// release binary, it wrapped and reported a fully covered level 30 as 4%.
fn coverage_percent(tiles: u64, total: u64) -> u64 {
	if total == 0 {
		return 0;
	}
	let percent = u128::from(tiles) * 100 / u128::from(total);
	u64::try_from(percent).unwrap_or(100)
}

/// Streams every tile's size, keeping totals, per-level totals and the biggest.
async fn scan_tile_sizes(source: &dyn TileSource, runtime: &TilesRuntime) -> Result<TileSizeReport> {
	let mut report = TileSizeReport::default();
	// Smallest size currently in `biggest_tiles`; anything below it can be
	// skipped without a binary search once the list is full.
	let mut min_size: u64 = 0;

	let tile_pyramid = source.tile_pyramid().await?;
	let progress = runtime.create_progress("scanning tiles", tile_pyramid.count_tiles());

	for bbox in tile_pyramid.to_iter_bboxes().filter(|b| !b.is_empty()) {
		let mut level = LevelSizeStats {
			level: bbox.level(),
			count: 0,
			size_sum: 0,
		};
		let mut stream = source.tile_size_stream(bbox).await?;
		while let Some((coord, size_u32)) = stream.next().await {
			let size = u64::from(size_u32);

			report.tile_count += 1;
			report.size_sum += size;
			level.count += 1;
			level.size_sum += size;
			progress.inc(1);

			if size < min_size {
				continue;
			}

			let pos = report
				.biggest_tiles
				.binary_search_by(|e| e.size.cmp(&size).reverse())
				.unwrap_or_else(|p| p);
			report.biggest_tiles.insert(
				pos,
				TileSizeEntry {
					size,
					x: coord.x,
					y: coord.y,
					z: coord.level,
				},
			);
			if report.biggest_tiles.len() > BIGGEST_TILES_LIMIT {
				report.biggest_tiles.pop();
			}
			min_size = report.biggest_tiles.last().expect("biggest_tiles is non-empty").size;
		}
		report.per_level.push(level);
	}
	progress.finish();

	Ok(report)
}

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
fn check_tile(mut tile: Tile) -> TileCheck {
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
	tiles: u64,
	stats: LayerStats,
}

/// Iterates every tile in the scan plan, running the MVT validator and per-layer
/// size breakdown on each. The per-tile decode+validate+measure (the expensive
/// part) runs in parallel across worker threads via `map_parallel`; aggregation
/// stays on this task so it needs no synchronization.
async fn scan_vector_contents(
	source: &dyn TileSource,
	runtime: &TilesRuntime,
	sample: Option<f64>,
) -> Result<VectorContentsReport> {
	let mut report = VectorContentsReport {
		sample_fraction: sample,
		..VectorContentsReport::default()
	};
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
			report.tiles_scanned += 1;
			progress.inc(1);

			let (issues, layers) = match check {
				TileCheck::DecodeFailed => {
					report.issues.decode_failures += 1;
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
			report.issues.tiles_with_issues += 1;

			for issue in &issues {
				report.issues.record(&issue.kind);
				if report.samples.len() < VALIDATION_SAMPLE_LIMIT {
					report.samples.push(IssueSample {
						z: coord.level,
						x: coord.x,
						y: coord.y,
						layer: issue.layer.clone(),
						feature_index: issue.feature_index,
						kind: describe_issue_kind(&issue.kind),
					});
				}
			}
		}
	}
	progress.finish();

	// Ordered here rather than at render time, so every consumer sees the same
	// sequence and none of them inherits a HashMap's iteration order.
	report.layer_sizes = size_agg
		.into_iter()
		.map(|((zoom, layer), agg)| LayerSizeEntry {
			zoom,
			layer,
			tiles: agg.tiles,
			stats: agg.stats,
		})
		.collect();
	report.layer_sizes.sort_by(|a, b| {
		a.zoom
			.cmp(&b.zoom)
			.then(b.stats.encoded_bytes.cmp(&a.stats.encoded_bytes))
			.then(a.layer.cmp(&b.layer))
	});

	Ok(report)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TilesRuntime;

	async fn berlin_report(depth: ProbeDepth) -> Result<ProbeReport> {
		let runtime = TilesRuntime::new_silent();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		probe_report(&**reader, depth, &runtime, None).await
	}

	#[tokio::test]
	async fn shallow_report_describes_the_container() -> Result<()> {
		let report = berlin_report(ProbeDepth::Shallow).await?;

		assert_eq!(report.tile_format, TileFormat::MVT);
		assert_eq!(report.tile_compression, TileCompression::Gzip);
		assert_eq!(report.zoom(), Some(0..=14));
		assert!(report.bbox().is_some());
		assert!(report.source_type.contains("mbtiles"));
		// Nothing deeper was asked for, so nothing deeper was computed.
		assert!(report.tile_sizes.is_none());
		assert!(report.contents.is_none());
		Ok(())
	}

	#[tokio::test]
	async fn a_raster_source_that_declares_no_size_gets_one_measured() -> Result<()> {
		use versatiles_core::{TileCompression, TilePyramid};

		use crate::{MockReader, TileSourceMetadata, Traversal};

		let runtime = TilesRuntime::new_silent();

		// The mock raster tiles are 256 px and its TileJSON declares nothing —
		// the shape of a container written before #247.
		let metadata = TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None);
		let reader = MockReader::new_mock(TilePyramid::new_full_up_to(3), metadata)?;
		let report = probe_report(&reader, ProbeDepth::Shallow, &runtime, None).await?;
		assert_eq!(report.tilejson.tile_size, None, "the source still declares nothing");
		assert_eq!(report.measured_tile_size.map(|s| s.size()), Some(256));

		// Once it declares a size, that is the answer and nothing is measured.
		let metadata = TileSourceMetadata::new(TileFormat::PNG, TileCompression::Uncompressed, Traversal::ANY, None);
		let mut reader = MockReader::new_mock(TilePyramid::new_full_up_to(3), metadata)?;
		reader.tilejson_mut().set_tile_size(512)?;
		let report = probe_report(&reader, ProbeDepth::Shallow, &runtime, None).await?;
		assert_eq!(report.tilejson.tile_size.map(|s| s.size()), Some(512));
		assert_eq!(report.measured_tile_size, None);

		// A vector source has no pixel size to measure.
		let report = berlin_report(ProbeDepth::Shallow).await?;
		assert_eq!(report.measured_tile_size, None);
		Ok(())
	}

	#[tokio::test]
	async fn pyramid_levels_are_ascending_and_non_empty() -> Result<()> {
		let report = berlin_report(ProbeDepth::Shallow).await?;

		assert!(!report.pyramid.is_empty());
		for pair in report.pyramid.windows(2) {
			assert!(pair[0].level < pair[1].level, "levels must ascend");
		}
		for level in &report.pyramid {
			assert!(level.tiles > 0, "empty levels must be filtered out");
			assert!(level.coverage_percent <= 100);
			assert!(
				level.coverage_percent > 0,
				"a level with tiles cannot cover 0% — a wrapped multiplication reads like this"
			);
			assert!(level.x_min <= level.x_max);
			assert!(level.y_min <= level.y_max);
		}
		Ok(())
	}

	#[tokio::test]
	async fn tile_sizes_are_consistent_with_their_levels() -> Result<()> {
		let report = berlin_report(ProbeDepth::TileSizes).await?;
		let sizes = report.tile_sizes.expect("TileSizes depth populates tile_sizes");

		assert!(sizes.tile_count > 0);
		assert_eq!(sizes.tile_count, sizes.per_level.iter().map(|l| l.count).sum::<u64>());
		assert_eq!(sizes.size_sum, sizes.per_level.iter().map(|l| l.size_sum).sum::<u64>());
		assert_eq!(sizes.average_size(), sizes.size_sum / sizes.tile_count);

		assert!(sizes.biggest_tiles.len() <= BIGGEST_TILES_LIMIT);
		for pair in sizes.biggest_tiles.windows(2) {
			assert!(pair[0].size >= pair[1].size, "biggest tiles must descend");
		}
		Ok(())
	}

	#[tokio::test]
	async fn contents_report_orders_layers_and_rolls_them_up() -> Result<()> {
		let report = berlin_report(ProbeDepth::TileContents).await?;
		let ContentsReport::Vector(contents) = report.contents.expect("populated at TileContents") else {
			panic!("berlin.mbtiles is a vector source");
		};

		assert!(contents.tiles_scanned > 0);
		assert!(!contents.layer_sizes.is_empty());
		assert_eq!(contents.sample_fraction, None);

		// The documented ordering: zoom ascending, then bytes descending.
		for pair in contents.layer_sizes.windows(2) {
			let ok = pair[0].zoom < pair[1].zoom
				|| (pair[0].zoom == pair[1].zoom && pair[0].stats.encoded_bytes >= pair[1].stats.encoded_bytes);
			assert!(ok, "unordered: {:?} then {:?}", pair[0], pair[1]);
		}

		// The roll-up covers exactly the same bytes as the per-zoom breakdown.
		let totals = contents.layer_totals();
		assert_eq!(
			totals.iter().map(|(_, s)| s.encoded_bytes).sum::<usize>(),
			contents.total_bytes()
		);
		for pair in totals.windows(2) {
			assert!(pair[0].1.encoded_bytes >= pair[1].1.encoded_bytes);
		}
		Ok(())
	}

	#[tokio::test]
	async fn sample_fraction_is_recorded_and_reduces_the_scan() -> Result<()> {
		let runtime = TilesRuntime::new_silent();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;

		let full = probe_report(&**reader, ProbeDepth::TileContents, &runtime, None).await?;
		let sampled = probe_report(&**reader, ProbeDepth::TileContents, &runtime, Some(0.1)).await?;

		let ContentsReport::Vector(full) = full.contents.unwrap() else {
			panic!("vector source")
		};
		let ContentsReport::Vector(sampled) = sampled.contents.unwrap() else {
			panic!("vector source")
		};

		assert_eq!(sampled.sample_fraction, Some(0.1));
		// Never more than the full scan. Not strictly fewer: the plan works in
		// 64x64 windows and always keeps at least one, so a fixture smaller than
		// a single window is covered entirely whatever fraction is asked for.
		assert!(
			sampled.tiles_scanned <= full.tiles_scanned,
			"sampling read {} but a full scan read {}",
			sampled.tiles_scanned,
			full.tiles_scanned
		);
		Ok(())
	}

	#[test]
	fn counters_split_issues_by_how_they_are_fixed() {
		let mut counters = ValidationCounters::default();
		counters.record(&IssueKind::MissingExtent);
		counters.record(&IssueKind::UnknownGeometryType);
		counters.decode_failures = 5;

		assert_eq!(counters.total_issues(), 2, "decode failures are not spec issues");
		assert_eq!(counters.fixable_automatically(), 1);
		assert_eq!(counters.needs_drop_offenders(), 1);
		assert_eq!(
			counters.by_kind().iter().map(|(_, n)| n).sum::<u64>(),
			counters.total_issues()
		);
	}

	#[test]
	fn coverage_survives_a_full_deep_level() {
		// A full level 29 holds 2^58 tiles, and `tiles * 100` passes u64::MAX
		// there. This panicked with overflow checks on, and — worse — wrapped
		// silently in the release binary, reporting a fully covered level 30 as
		// 4%. Not a hypothetical depth: `from_debug` advertises levels 0-30.
		for level in 0..=30u8 {
			let tiles = 1u64 << (2 * u32::from(level));
			assert_eq!(coverage_percent(tiles, tiles), 100, "full level {level}");
			if level > 0 {
				// Level 0 holds a single tile, so it has no half.
				assert_eq!(coverage_percent(tiles / 2, tiles), 50, "half of level {level}");
			}
		}
	}

	#[test]
	fn coverage_of_nothing_is_zero_rather_than_a_division_by_zero() {
		assert_eq!(coverage_percent(0, 0), 0);
	}
}
