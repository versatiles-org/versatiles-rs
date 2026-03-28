//! Two-pass tile assembly: scan → write opaque → batch-composite translucent.
//!
//! # How it works
//!
//! **First pass** — streams every source once:
//! - **Opaque** tiles are written directly to the sink and recorded in `done`.
//! - **Empty** tiles are skipped.
//! - **Translucent** tiles are recorded as `(TileCoord, Vec<source_index>)`.
//!
//! **Between passes** — coords already in `done` are removed, the rest are sorted
//! by `(level, x, y)` and split into batches bounded by `--max-buffer-size`.
//!
//! **Second pass** — for each batch, only the needed sources are opened. Tiles are
//! composited onto a `TranslucentBuffer` and flushed to the sink once the batch is
//! complete.

use super::translucent_buffer::TranslucentBuffer;
use anyhow::{Context, Result, anyhow, ensure};
use futures::{StreamExt, future::ready};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use versatiles_container::{Tile, TileSink, TilesRuntime, open_tile_sink};
use versatiles_core::{
	Blob, ConcurrencyLimits, TileCompression, TileCoord, TileFormat, TileJSON, TileStream, utils::HilbertIndex,
};
use versatiles_image::traits::DynamicImageTraitOperation;

/// CLI arguments for `mosaic assemble`.
#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Assemble {
	/// Input container paths, URLs, or glob patterns, followed by the output path.
	/// The last argument is always the output; all preceding arguments are inputs.
	/// Glob patterns (*, ?, [) are expanded. Containers listed earlier overlay
	/// containers listed later. Arguments ending in .txt are read as list files
	/// (one path per line, # comments supported). Use @filename to read any file
	/// as a list regardless of extension.
	#[arg(required = true, num_args = 2..)]
	paths: Vec<String>,

	/// Lossy WebP quality for the final output tiles, using zoom-dependent syntax
	/// (e.g. "70,14:50,15:20"). Default: 75.
	#[arg(long, value_name = "str", default_value = "75")]
	quality: String,

	/// Encode translucent tiles as lossless WebP instead of using the lossy --quality setting
	#[arg(long)]
	lossless: bool,

	/// Minimum zoom level to include in the output (default: include all).
	#[arg(long, value_name = "int")]
	min_zoom: Option<u8>,

	/// Maximum zoom level to include in the output (default: include all).
	#[arg(long, value_name = "int")]
	max_zoom: Option<u8>,

	/// Maximum memory for the tile buffer (default: 4g).
	/// Supports units: k, m, g, t (e.g. "4g") and % of system memory (e.g. "50%").
	/// Plain number is interpreted as bytes. 0 means unlimited.
	#[arg(long, value_name = "size", default_value = "4g")]
	max_buffer_size: String,
}

/// Encoding configuration shared across assemble functions.
#[derive(Clone)]
struct AssembleConfig {
	quality: [Option<u8>; 32],
	lossless: bool,
	tile_format: TileFormat,
	tile_compression: TileCompression,
}

// ─── CLI parsing helpers ───

fn parse_quality(quality: &str) -> Result<[Option<u8>; 32]> {
	let mut result: [Option<u8>; 32] = [None; 32];
	let mut zoom: i32 = -1;
	for part in quality.split(',') {
		let mut part = part.trim();
		zoom += 1;
		if part.is_empty() {
			continue;
		}
		if let Some(idx) = part.find(':') {
			zoom = part[0..idx].trim().parse()?;
			ensure!((0..=31).contains(&zoom), "Zoom level must be between 0 and 31");
			part = &part[(idx + 1)..];
		}
		let quality_val: u8 = part.trim().parse()?;
		ensure!(quality_val <= 100, "Quality value must be between 0 and 100");
		for z in zoom..32 {
			result[usize::try_from(z).unwrap()] = Some(quality_val);
		}
	}
	Ok(result)
}

fn parse_input_list(content: &str) -> Vec<String> {
	content
		.lines()
		.map(|line| {
			let line = if let Some(idx) = line.find('#') {
				&line[..idx]
			} else {
				line
			};
			line.trim().to_string()
		})
		.filter(|line| !line.is_empty())
		.collect()
}

fn resolve_inputs(args: &[String]) -> Result<Vec<String>> {
	let mut result = Vec::new();
	for arg in args {
		if let Some(file_path) = arg.strip_prefix('@') {
			let content = std::fs::read_to_string(file_path)
				.with_context(|| format!("Failed to read input list file: {file_path}"))?;
			result.extend(parse_input_list(&content));
		} else if std::path::Path::new(arg)
			.extension()
			.is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
		{
			let content =
				std::fs::read_to_string(arg).with_context(|| format!("Failed to read input list file: {arg}"))?;
			result.extend(parse_input_list(&content));
		} else if arg.contains('*') || arg.contains('?') || arg.contains('[') {
			let mut matches: Vec<String> = Vec::new();
			for entry in glob::glob(arg).with_context(|| format!("Invalid glob pattern: {arg}"))? {
				let path = entry.with_context(|| format!("Error reading glob result for: {arg}"))?;
				matches.push(path.to_string_lossy().into_owned());
			}
			ensure!(!matches.is_empty(), "Glob pattern matched no files: {arg}");
			matches.sort();
			result.extend(matches);
		} else {
			result.push(arg.clone());
		}
	}
	Ok(result)
}

fn parse_buffer_size(s: &str) -> Result<u64> {
	let s = s.trim();
	if s == "0" {
		return Ok(0);
	}

	if let Some(pct) = s.strip_suffix('%') {
		let pct: f64 = pct
			.trim()
			.parse()
			.with_context(|| format!("Invalid percentage in buffer size: {s}"))?;
		ensure!(
			(0.0..=100.0).contains(&pct),
			"Buffer size percentage must be between 0 and 100, got {pct}"
		);
		let total = total_system_memory()?;
		#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
		return Ok((total as f64 * pct / 100.0) as u64);
	}

	let s_lower = s.to_ascii_lowercase();
	let s_unit = s_lower.strip_suffix('b').unwrap_or(&s_lower);
	let (num_str, multiplier) = if let Some(n) = s_unit.strip_suffix('t') {
		(n, 1_000_000_000_000u64)
	} else if let Some(n) = s_unit.strip_suffix('g') {
		(n, 1_000_000_000)
	} else if let Some(n) = s_unit.strip_suffix('m') {
		(n, 1_000_000)
	} else if let Some(n) = s_unit.strip_suffix('k') {
		(n, 1_000)
	} else {
		(s_lower.as_str(), 1)
	};

	let num: f64 = num_str
		.trim()
		.parse()
		.with_context(|| format!("Invalid buffer size: {s}"))?;
	ensure!(num >= 0.0, "Buffer size must not be negative: {s}");
	#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
	Ok((num * multiplier as f64) as u64)
}

fn total_system_memory() -> Result<u64> {
	#[cfg(target_os = "macos")]
	{
		let output = std::process::Command::new("sysctl")
			.args(["-n", "hw.memsize"])
			.output()
			.context("failed to run sysctl")?;
		ensure!(output.status.success(), "sysctl hw.memsize failed");
		let s = String::from_utf8_lossy(&output.stdout);
		s.trim().parse::<u64>().context("failed to parse hw.memsize")
	}
	#[cfg(target_os = "linux")]
	{
		let content = std::fs::read_to_string("/proc/meminfo").context("failed to read /proc/meminfo")?;
		for line in content.lines() {
			if let Some(rest) = line.strip_prefix("MemTotal:") {
				let kb_str = rest.trim().trim_end_matches("kB").trim();
				let kb: u64 = kb_str.parse().context("failed to parse MemTotal")?;
				return Ok(kb * 1024);
			}
		}
		anyhow::bail!("MemTotal not found in /proc/meminfo")
	}
	#[cfg(not(any(target_os = "macos", target_os = "linux")))]
	{
		anyhow::bail!("Cannot detect system memory on this platform; use an absolute size instead of %")
	}
}

// ─── Tile processing helpers ───
//
// Encoding requirements:
//
// - **Opaque tiles** must never be re-encoded. Their original blob is written
//   to the sink byte-for-byte (only recompressed if the container compression
//   differs, which `into_blob` handles as a no-op when it already matches).
//
// - **Translucent tiles** are re-encoded exactly once as lossy WebP (or
//   lossless when `--lossless` is set). The single encoding happens during
//   the flush step in `encode_tiles_parallel`, which calls `change_format`
//   to set format + quality, followed by `into_blob` → `materialize_blob`
//   to produce the blob. Compositing in `composite_two_tiles` deliberately
//   does NOT encode — it keeps the merged image as raw content so that the
//   flush step is the only place where lossy compression is applied.

fn validate_source_format(
	path: &str,
	metadata: &versatiles_container::TileSourceMetadata,
	config: &AssembleConfig,
) -> Result<()> {
	ensure!(
		metadata.tile_format == config.tile_format,
		"Source {path} has tile format {:?}, expected {:?}",
		metadata.tile_format,
		config.tile_format
	);
	ensure!(
		metadata.tile_compression == config.tile_compression,
		"Source {path} has tile compression {:?}, expected {:?}",
		metadata.tile_compression,
		config.tile_compression
	);
	Ok(())
}

/// Composite two tiles using additive alpha blending (`base` on bottom, `top` on top).
///
/// Returns the merged tile with raw image content (no blob, no encoding).
/// Encoding is deferred to `encode_tiles_parallel` so that lossy compression
/// is applied exactly once.
fn composite_two_tiles(base: Tile, top: Tile) -> Result<Tile> {
	let base_image = base.into_image()?;
	let top_image = top.into_image()?;

	let mut result = base_image;
	result.overlay_additive(&top_image)?;

	// Keep as raw image — `encode_tiles_parallel` will set format + quality later.
	Tile::from_image(result, TileFormat::WEBP)
}

/// Write an opaque tile's original blob to the sink without re-encoding.
fn write_opaque_blob(tile: Tile, config: &AssembleConfig) -> Result<Blob> {
	tile.into_blob(config.tile_compression)
}

/// Re-encode translucent tiles as WebP in parallel and compress for the output container.
///
/// This is the single place where lossy (or lossless) WebP compression is applied.
/// Tiles coming from `composite_two_tiles` carry raw image content (no blob),
/// so `change_format` + `into_blob` produces the one-and-only encoded blob.
/// Single-source tiles still hold their original source blob, which is decoded
/// and re-encoded here as well.
fn encode_tiles_parallel(tiles: Vec<(TileCoord, Tile)>, config: &AssembleConfig) -> Vec<Result<(TileCoord, Blob)>> {
	let config = config.clone();
	let chunk_size = ConcurrencyLimits::default().cpu_bound;
	let mut results = Vec::with_capacity(tiles.len());
	let mut iter = tiles.into_iter().peekable();
	while iter.peek().is_some() {
		let chunk: Vec<_> = iter.by_ref().take(chunk_size).collect();
		let chunk_results: Vec<_> = std::thread::scope(|s| {
			let handles: Vec<_> = chunk
				.into_iter()
				.map(|(coord, mut tile)| {
					let cfg = &config;
					s.spawn(move || {
						let quality = if cfg.lossless {
							Some(100)
						} else {
							cfg.quality[coord.level as usize]
						};
						tile.change_format(TileFormat::WEBP, quality, None)?;
						Ok((coord, tile.into_blob(cfg.tile_compression)?))
					})
				})
				.collect();
			handles.into_iter().map(|h| h.join().unwrap()).collect()
		});
		results.extend(chunk_results);
	}
	results
}

// ─── Entry point ───

pub async fn run(args: &Assemble, runtime: &TilesRuntime) -> Result<()> {
	ensure!(args.paths.len() >= 2, "Need at least one input and one output path");
	let (input_args, output) = args.paths.split_at(args.paths.len() - 1);
	let output = &output[0];

	log::info!("mosaic assemble to {output:?}");

	let paths = resolve_inputs(input_args)?;
	ensure!(!paths.is_empty(), "No input container paths resolved");

	let quality = parse_quality(&args.quality)?;
	let max_buffer_size = parse_buffer_size(&args.max_buffer_size)?;

	log::info!("assembling {} containers (two-pass)", paths.len());

	assemble_two_pass(
		output,
		&paths,
		&quality,
		args.lossless,
		args.min_zoom,
		args.max_zoom,
		max_buffer_size,
		runtime,
	)
	.await
}

// ─── Tile fetching ───

/// Read all tiles for a given source that are relevant to the batch.
///
/// Returns `(coord, tile)` pairs with empty tiles already filtered out.
/// Used both for direct fetching and for pre-fetching the next source.
async fn fetch_source_tiles(
	source_idx: usize,
	batch: &[(TileCoord, Vec<usize>)],
	paths: &[String],
	runtime: &TilesRuntime,
) -> Result<Vec<(TileCoord, Tile)>> {
	let path = &paths[source_idx];
	let reader = runtime
		.get_reader_from_str(path)
		.await
		.with_context(|| format!("Failed to open container: {path}"))?;

	let coords: Vec<TileCoord> = batch
		.iter()
		.filter(|(_, srcs)| srcs.contains(&source_idx))
		.map(|(coord, _)| *coord)
		.collect();

	let concurrency = ConcurrencyLimits::default().io_bound;
	let tiles: Vec<Result<Option<(TileCoord, Tile)>>> = futures::stream::iter(coords)
		.map(|coord| {
			let reader = reader.clone();
			async move {
				match reader.get_tile(&coord).await? {
					Some(tile) => Ok(Some((coord, tile))),
					None => Ok(None),
				}
			}
		})
		.buffer_unordered(concurrency)
		.collect()
		.await;

	// Filter empty tiles outside the async executor to avoid blocking on potential image decode
	let mut result = Vec::with_capacity(tiles.len());
	for tile_result in tiles {
		if let Some((coord, mut tile)) = tile_result?
			&& !tile.is_empty()?
		{
			result.push((coord, tile));
		}
	}
	Ok(result)
}

// ─── PCA-based batch partitioning ───

/// A group of tiles sharing the same source-set signature.
struct SignatureGroup {
	sources: Vec<usize>,    // sorted source indices (the signature)
	coords: Vec<TileCoord>, // all tiles with this signature
}

/// Collapse `translucent_map` into signature groups by grouping on the source list.
fn collapse_into_signature_groups(translucent_map: HashMap<TileCoord, Vec<usize>>) -> Vec<SignatureGroup> {
	let mut sig_map: HashMap<Vec<usize>, Vec<TileCoord>> = HashMap::new();
	for (coord, mut sources) in translucent_map {
		sources.sort_unstable();
		sig_map.entry(sources).or_default().push(coord);
	}
	sig_map
		.into_iter()
		.map(|(sources, coords)| SignatureGroup { sources, coords })
		.collect()
}

/// Recursively partition signature groups into batches of at most `batch_size` tiles.
fn partition_into_batches(
	groups: Vec<SignatureGroup>,
	num_sources: usize,
	batch_size: usize,
) -> Vec<Vec<SignatureGroup>> {
	let total_tiles: usize = groups.iter().map(|g| g.coords.len()).sum();

	// Base case: fits in one batch
	if total_tiles <= batch_size {
		return vec![groups];
	}

	// Special case: single group that exceeds batch_size — split coords spatially
	if groups.len() == 1 {
		let g = groups.into_iter().next().unwrap();
		let mut coords = g.coords;
		// Sort spatially for locality
		coords.sort_by(|a, b| a.level.cmp(&b.level).then(a.x.cmp(&b.x)).then(a.y.cmp(&b.y)));
		let mut batches = Vec::new();
		for chunk in coords.chunks(batch_size) {
			batches.push(vec![SignatureGroup {
				sources: g.sources.clone(),
				coords: chunk.to_vec(),
			}]);
		}
		return batches;
	}

	// PCA step: find the principal component via power iteration

	// Build source membership sets for efficient lookup
	let source_sets: Vec<HashSet<usize>> = groups.iter().map(|g| g.sources.iter().copied().collect()).collect();

	// Compute weighted mean
	let total_weight: f64 = groups.iter().map(|g| g.coords.len() as f64).sum();
	let mut mean = vec![0.0f64; num_sources];
	for (i, g) in groups.iter().enumerate() {
		let w = g.coords.len() as f64;
		for (s, m) in mean.iter_mut().enumerate() {
			if source_sets[i].contains(&s) {
				*m += w;
			}
		}
	}
	for m in &mut mean {
		*m /= total_weight;
	}

	// Power iteration to find PC1
	let mut v: Vec<f64> = (1..=num_sources).map(|i| i as f64).collect();
	let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
	for x in &mut v {
		*x /= norm;
	}

	for _ in 0..15 {
		let mut new_v = vec![0.0f64; num_sources];
		for (i, g) in groups.iter().enumerate() {
			let w = g.coords.len() as f64;
			// dot = (x_i - μ) · v
			let dot: f64 = (0..num_sources)
				.map(|s| {
					let x = if source_sets[i].contains(&s) { 1.0 } else { 0.0 };
					(x - mean[s]) * v[s]
				})
				.sum();
			// accumulate: new_v += w * dot * (x_i - μ)
			for s in 0..num_sources {
				let x = if source_sets[i].contains(&s) { 1.0 } else { 0.0 };
				new_v[s] += w * dot * (x - mean[s]);
			}
		}
		let n = new_v.iter().map(|x| x * x).sum::<f64>().sqrt();
		if n > 0.0 {
			for x in &mut new_v {
				*x /= n;
			}
		}
		v = new_v;
	}

	// Project each group onto PC1 and sort by score
	let mut scored: Vec<(f64, usize)> = groups
		.iter()
		.enumerate()
		.map(|(i, _)| {
			let score: f64 = (0..num_sources)
				.map(|s| {
					let x = if source_sets[i].contains(&s) { 1.0 } else { 0.0 };
					(x - mean[s]) * v[s]
				})
				.sum();
			(score, i)
		})
		.collect();
	scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

	// Find the split point: balanced by tile count
	let half = total_tiles / 2;
	let mut acc = 0usize;
	let mut split_pos = 1; // at least 1 group on the left
	for (pos, &(_, idx)) in scored.iter().enumerate() {
		acc += groups[idx].coords.len();
		if acc >= half {
			split_pos = pos + 1;
			break;
		}
	}
	// Ensure at least one group on each side
	split_pos = split_pos.clamp(1, scored.len() - 1);

	// Split into two halves
	let indices_left: Vec<usize> = scored[..split_pos].iter().map(|&(_, i)| i).collect();
	let indices_right: Vec<usize> = scored[split_pos..].iter().map(|&(_, i)| i).collect();

	// Move groups into left/right by index
	// We need to consume `groups`, so collect into an indexed structure
	let mut groups_by_idx: Vec<Option<SignatureGroup>> = groups.into_iter().map(Some).collect();

	let left: Vec<SignatureGroup> = indices_left
		.into_iter()
		.map(|i| groups_by_idx[i].take().unwrap())
		.collect();
	let right: Vec<SignatureGroup> = indices_right
		.into_iter()
		.map(|i| groups_by_idx[i].take().unwrap())
		.collect();

	// Recurse
	let mut batches = partition_into_batches(left, num_sources, batch_size);
	batches.extend(partition_into_batches(right, num_sources, batch_size));
	batches
}

// ─── Two-pass pipeline ───

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn assemble_two_pass(
	output: &str,
	paths: &[String],
	quality: &[Option<u8>; 32],
	lossless: bool,
	min_zoom: Option<u8>,
	max_zoom: Option<u8>,
	max_buffer_size: u64,
	runtime: &TilesRuntime,
) -> Result<()> {
	// ─── First pass: scan sources + write opaque tiles ───
	//
	// Each source is opened once. We extract its pyramid (for tilejson metadata
	// and zoom clipping) and stream its tiles in the same iteration—no separate
	// prescan needed.

	let mut config: Option<AssembleConfig> = None;
	let mut tilejson: Option<TileJSON> = None;
	let mut tile_dim: u64 = 256;
	let mut sink: Option<Arc<Box<dyn TileSink>>> = None;

	let mut translucent_map: HashMap<TileCoord, Vec<usize>> = HashMap::new();
	let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();

	let progress = runtime.create_progress("pass 1/2: scanning sources", paths.len() as u64);

	for (idx, path) in paths.iter().enumerate() {
		let reader = runtime
			.get_reader_from_str(path)
			.await
			.with_context(|| format!("Failed to open container: {path}"))?;

		let metadata = reader.metadata();
		let reader_tilejson = reader.tilejson();

		// First source: discover format and open sink
		let cfg = if let Some(ref cfg) = config {
			validate_source_format(path, metadata, cfg)?;
			cfg.clone()
		} else {
			let cfg = AssembleConfig {
				quality: *quality,
				lossless,
				tile_format: metadata.tile_format,
				tile_compression: metadata.tile_compression,
			};
			tile_dim = reader_tilejson.tile_size.map_or(256, |ts| u64::from(ts.size()));
			tilejson = Some(TileJSON {
				tile_format: Some(cfg.tile_format),
				tile_type: Some(cfg.tile_format.to_type()),
				tile_schema: reader_tilejson.tile_schema,
				tile_size: reader_tilejson.tile_size,
				..TileJSON::default()
			});
			sink = Some(Arc::new(open_tile_sink(
				output,
				cfg.tile_format,
				cfg.tile_compression,
				runtime,
			)?));
			config = Some(cfg.clone());
			cfg
		};

		tilejson.as_mut().unwrap().merge(reader_tilejson)?;

		// Get pyramid and clip to zoom range
		let mut pyramid = metadata.bbox_pyramid.clone();
		if let Some(min) = min_zoom {
			pyramid.set_level_min(min);
		}
		if let Some(max) = max_zoom {
			pyramid.set_level_max(max);
		}
		if pyramid.is_empty() {
			progress.inc(1);
			continue;
		}

		let sink_arc = sink.as_ref().unwrap();

		// Stream all tiles from this source
		let level_bboxes: Vec<_> = pyramid.iter_levels().filter(|b| !b.is_empty()).copied().collect();
		let streams: Vec<TileStream<'static, Tile>> =
			futures::future::try_join_all(level_bboxes.into_iter().map(|bbox| reader.get_tile_stream(bbox))).await?;
		let combined = streams
			.into_iter()
			.reduce(TileStream::chain)
			.unwrap_or_else(TileStream::empty);

		// Classify tiles: opaque → write, empty → skip, translucent → record
		let concurrency = ConcurrencyLimits::default().cpu_bound * 2;
		let sink_ref = Arc::clone(sink_arc);
		let done_ref = Arc::clone(&done);
		let config_clone = cfg;
		let translucent_coords: Arc<Mutex<Vec<TileCoord>>> = Arc::default();
		let translucent_coords_ref = Arc::clone(&translucent_coords);

		let callback = Arc::new(move |coord: TileCoord, mut tile: Tile| -> Result<()> {
			// Skip tiles already written
			if done_ref.lock().unwrap().contains(&coord) {
				return Ok(());
			}

			if tile.is_empty()? {
				return Ok(());
			}

			if tile.is_opaque()? {
				// Opaque: write original blob without re-encoding.
				let blob = write_opaque_blob(tile, &config_clone)?;
				sink_ref.write_tile(&coord, &blob)?;
				done_ref.lock().unwrap().insert(coord);
			} else {
				// Translucent: record for second-pass compositing.
				translucent_coords_ref.lock().unwrap().push(coord);
			}

			Ok(())
		});

		let mut result = Ok(());
		combined
			.inner
			.map(move |(coord, item)| {
				let cb = Arc::clone(&callback);
				tokio::task::spawn_blocking(move || (coord, cb(coord, item)))
			})
			.buffer_unordered(concurrency)
			.for_each(|task_result| {
				match task_result {
					Ok((coord, Err(e))) if result.is_ok() => {
						result = Err(e.context(format!("Failed to process tile at {coord:?}")));
					}
					Err(e) => panic!("Spawned task panicked: {e}"),
					_ => {}
				}
				ready(())
			})
			.await;
		result?;

		// Record translucent coords for this source
		let coords = std::mem::take(&mut *translucent_coords.lock().unwrap());
		for coord in coords {
			translucent_map.entry(coord).or_default().push(idx);
		}

		progress.inc(1);
	}

	progress.finish();

	let config = config.ok_or_else(|| anyhow!("no sources found"))?;
	let tilejson = tilejson.unwrap();
	let sink = sink.unwrap();

	// ─── Between passes: prepare batches ───

	// Remove coords already written as opaque
	{
		let done_set = done.lock().unwrap();
		translucent_map.retain(|coord, _| !done_set.contains(coord));
	}
	drop(done);

	if translucent_map.is_empty() {
		// All tiles were opaque — we're done
		let sink = Arc::try_unwrap(sink).map_err(|_| anyhow!("sink still has references"))?;
		sink.finish(&tilejson, runtime)?;
		log::info!("finished mosaic assemble (all tiles opaque, no second pass needed)");
		return Ok(());
	}

	// Collapse into signature groups and partition via PCA-based recursive bisection
	let total_tiles = translucent_map.len();
	let groups = collapse_into_signature_groups(translucent_map);
	let num_sources = paths.len();

	let batch_size = if max_buffer_size > 0 {
		usize::try_from((max_buffer_size / (tile_dim * tile_dim * 4)).max(1)).unwrap_or(usize::MAX)
	} else {
		total_tiles
	};

	let batch_groups = partition_into_batches(groups, num_sources, batch_size);

	// Convert back to the format the second pass expects
	let batches: Vec<Vec<(TileCoord, Vec<usize>)>> = batch_groups
		.into_iter()
		.map(|batch| {
			batch
				.into_iter()
				.flat_map(|g| {
					let sources = g.sources;
					g.coords.into_iter().map(move |c| (c, sources.clone()))
				})
				.collect()
		})
		.collect();

	// ─── Second pass: composite translucent batches ───

	let total_source_opens: usize = batches
		.iter()
		.map(|b| b.iter().flat_map(|(_, s)| s).collect::<BTreeSet<_>>().len())
		.sum();
	log::info!(
		"partitioned {} tiles into {} batches ({} total source-opens)",
		total_tiles,
		batches.len(),
		total_source_opens
	);

	let total_source_reads: u64 = batches
		.iter()
		.map(|batch| {
			let sources: BTreeSet<usize> = batch.iter().flat_map(|(_, srcs)| srcs).copied().collect();
			sources.len() as u64
		})
		.sum();

	let progress = runtime.create_progress("pass 2/2: compositing tiles", total_source_reads);

	let concurrency = ConcurrencyLimits::default().cpu_bound;

	for batch in &batches {
		let sources_needed: Vec<usize> = {
			let set: BTreeSet<usize> = batch.iter().flat_map(|(_, srcs)| srcs).copied().collect();
			set.into_iter().collect()
		};

		let buffer = Arc::new(TranslucentBuffer::new());

		// Pre-fetch tiles for the first source while we set up
		let mut prefetched: Option<Vec<(TileCoord, Tile)>> = None;

		let batch_arc = Arc::new(batch.clone());
		let paths_arc = Arc::new(paths.to_vec());

		for (pos, &source_idx) in sources_needed.iter().enumerate() {
			// Use pre-fetched tiles if available, otherwise fetch now
			let fetched_tiles = if let Some(tiles) = prefetched.take() {
				tiles
			} else {
				fetch_source_tiles(source_idx, batch, paths, runtime).await?
			};

			// Kick off pre-fetch for the NEXT source in the background while
			// we composite the current one. This overlaps I/O with CPU work.
			let prefetch_handle = if let Some(&next_idx) = sources_needed.get(pos + 1) {
				let batch_clone = Arc::clone(&batch_arc);
				let paths_clone = Arc::clone(&paths_arc);
				let runtime = runtime.clone();
				Some(tokio::spawn(async move {
					fetch_source_tiles(next_idx, &batch_clone, &paths_clone, &runtime).await
				}))
			} else {
				None
			};

			// Composite fetched tiles into the buffer in parallel
			let buffer_ref = Arc::clone(&buffer);
			let callback = Arc::new(move |coord: TileCoord, tile: Tile| -> Result<()> {
				let key = coord.get_hilbert_index()?;
				let existing = buffer_ref.remove(key);
				if let Some((_, existing_tile)) = existing {
					let merged = composite_two_tiles(tile, existing_tile)?;
					buffer_ref.insert(coord, merged)?;
				} else {
					buffer_ref.insert(coord, tile)?;
				}
				Ok(())
			});

			let mut result = Ok(());
			futures::stream::iter(fetched_tiles)
				.map(|(coord, tile)| {
					let cb = Arc::clone(&callback);
					tokio::task::spawn_blocking(move || {
						cb(coord, tile)?;
						Ok::<_, anyhow::Error>(())
					})
				})
				.buffer_unordered(concurrency)
				.for_each(|task_result| {
					match task_result {
						Ok(Err(e)) if result.is_ok() => {
							result = Err(e);
						}
						Err(e) => panic!("Spawned task panicked: {e}"),
						_ => {}
					}
					ready(())
				})
				.await;
			result?;

			// Await the pre-fetched tiles for the next source
			if let Some(handle) = prefetch_handle {
				prefetched = Some(handle.await.map_err(|e| anyhow!("prefetch task panicked: {e}"))??);
			}

			progress.inc(1);
		}

		// Flush buffer: encode translucent tiles to WebP (single encoding step)
		let buffer = Arc::try_unwrap(buffer).map_err(|_| anyhow!("buffer still has references"))?;
		let tiles: Vec<_> = buffer.drain().into_values().collect();
		if !tiles.is_empty() {
			let prepared = encode_tiles_parallel(tiles, &config);
			for result in prepared {
				let (coord, blob) = result?;
				sink.write_tile(&coord, &blob)?;
			}
		}
	}

	progress.finish();

	let sink = Arc::try_unwrap(sink).map_err(|_| anyhow!("sink still has references"))?;
	sink.finish(&tilejson, runtime)?;

	log::info!("finished mosaic assemble");
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_input_list() {
		let content = "
# This is a comment
tiles/001.versatiles
tiles/002.versatiles

  tiles/003.versatiles
# Another comment
https://example.com/tiles/004.versatiles
";
		let paths = parse_input_list(content);
		assert_eq!(
			paths,
			vec![
				"tiles/001.versatiles",
				"tiles/002.versatiles",
				"tiles/003.versatiles",
				"https://example.com/tiles/004.versatiles",
			]
		);
	}

	#[test]
	fn test_parse_input_list_inline_comments() {
		let content = "tiles/001.versatiles # first container\ntiles/002.versatiles";
		let paths = parse_input_list(content);
		assert_eq!(paths, vec!["tiles/001.versatiles", "tiles/002.versatiles"]);
	}

	#[test]
	fn test_parse_input_list_empty() {
		let content = "\n# only comments\n  \n";
		let paths = parse_input_list(content);
		assert!(paths.is_empty());
	}

	#[test]
	fn test_parse_buffer_size() {
		// Plain bytes
		assert_eq!(parse_buffer_size("0").unwrap(), 0);
		assert_eq!(parse_buffer_size("1024").unwrap(), 1024);

		// Units (case-insensitive)
		assert_eq!(parse_buffer_size("1k").unwrap(), 1_000);
		assert_eq!(parse_buffer_size("1K").unwrap(), 1_000);
		assert_eq!(parse_buffer_size("1Kb").unwrap(), 1_000);
		assert_eq!(parse_buffer_size("1t").unwrap(), 1_000_000_000_000);
		assert_eq!(parse_buffer_size("2m").unwrap(), 2_000_000);
		assert_eq!(parse_buffer_size("2M").unwrap(), 2_000_000);
		assert_eq!(parse_buffer_size("2mB").unwrap(), 2_000_000);
		assert_eq!(parse_buffer_size("3g").unwrap(), 3_000_000_000);
		assert_eq!(parse_buffer_size("3G").unwrap(), 3_000_000_000);
		assert_eq!(parse_buffer_size("3gb").unwrap(), 3_000_000_000);

		// Fractional with unit
		assert_eq!(parse_buffer_size("1.5g").unwrap(), 1_500_000_000);
		assert_eq!(parse_buffer_size("0.5m").unwrap(), 500_000);

		// Whitespace
		assert_eq!(parse_buffer_size("  4g  ").unwrap(), 4_000_000_000);
		assert_eq!(parse_buffer_size("2 m").unwrap(), 2_000_000);

		// Percentage (only on platforms where total_system_memory() works)
		#[cfg(any(target_os = "macos", target_os = "linux"))]
		{
			let result = parse_buffer_size("50%").unwrap();
			assert!(result > 0, "50% of system memory should be > 0");
		}
		#[cfg(not(any(target_os = "macos", target_os = "linux")))]
		{
			assert!(parse_buffer_size("50%").is_err());
		}

		// Errors
		assert!(parse_buffer_size("abc").is_err());
		assert!(parse_buffer_size("-1g").is_err());
		assert!(parse_buffer_size("101%").is_err());
	}

	#[test]
	fn test_parse_quality() {
		let q = parse_quality("80").unwrap();
		assert_eq!(q[0], Some(80));
		assert_eq!(q[15], Some(80));

		let q = parse_quality("80,70,14:50,15:20").unwrap();
		assert_eq!(q[0], Some(80));
		assert_eq!(q[1], Some(70));
		assert_eq!(q[13], Some(70));
		assert_eq!(q[14], Some(50));
		assert_eq!(q[15], Some(20));
	}

	#[test]
	fn test_resolve_inputs_literal() {
		let args = vec!["a.versatiles".to_string(), "b.versatiles".to_string()];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result, vec!["a.versatiles", "b.versatiles"]);
	}

	#[test]
	fn test_resolve_inputs_at_file() {
		let dir = tempfile::tempdir().unwrap();
		let list_path = dir.path().join("inputs.txt");
		std::fs::write(&list_path, "one.versatiles\ntwo.versatiles\n# comment\n").unwrap();

		let args = vec![format!("@{}", list_path.display())];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result, vec!["one.versatiles", "two.versatiles"]);
	}

	#[test]
	fn test_resolve_inputs_glob() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("a.versatiles"), "").unwrap();
		std::fs::write(dir.path().join("b.versatiles"), "").unwrap();
		std::fs::write(dir.path().join("c.txt"), "").unwrap();

		let pattern = format!("{}/*.versatiles", dir.path().display());
		let args = vec![pattern];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result.len(), 2);
		assert!(result[0].ends_with("a.versatiles"));
		assert!(result[1].ends_with("b.versatiles"));
	}

	#[test]
	fn test_resolve_inputs_txt_file() {
		let dir = tempfile::tempdir().unwrap();
		let list_path = dir.path().join("sources.txt");
		std::fs::write(&list_path, "alpha.versatiles\n# skip\nbeta.versatiles\n").unwrap();

		let args = vec![list_path.to_string_lossy().into_owned()];
		let result = resolve_inputs(&args).unwrap();
		assert_eq!(result, vec!["alpha.versatiles", "beta.versatiles"]);
	}

	#[test]
	fn test_resolve_inputs_glob_no_match() {
		let args = vec!["/nonexistent_dir_xyz/*.versatiles".to_string()];
		assert!(resolve_inputs(&args).is_err());
	}

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord { level, x, y }
	}

	#[test]
	fn test_collapse_into_signature_groups() {
		let mut map: HashMap<TileCoord, Vec<usize>> = HashMap::new();
		map.insert(tc(0, 0, 0), vec![1, 0]);
		map.insert(tc(0, 1, 0), vec![0, 1]);
		map.insert(tc(0, 2, 0), vec![2, 3]);

		let groups = collapse_into_signature_groups(map);
		// Two unique signatures: [0,1] and [2,3]
		assert_eq!(groups.len(), 2);

		for g in &groups {
			if g.sources == vec![0, 1] {
				assert_eq!(g.coords.len(), 2);
			} else if g.sources == vec![2, 3] {
				assert_eq!(g.coords.len(), 1);
			} else {
				panic!("unexpected signature: {:?}", g.sources);
			}
		}
	}

	#[test]
	fn test_partition_single_group_over_batch_size() {
		let coords: Vec<TileCoord> = (0..10).map(|i| tc(0, i, 0)).collect();
		let groups = vec![SignatureGroup {
			sources: vec![0, 1],
			coords,
		}];

		let batches = partition_into_batches(groups, 4, 3);
		// 10 tiles, batch_size=3 → 4 batches (3+3+3+1)
		assert_eq!(batches.len(), 4);

		let total: usize = batches.iter().flat_map(|b| b.iter()).map(|g| g.coords.len()).sum();
		assert_eq!(total, 10);

		for batch in &batches {
			let batch_tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert!(batch_tiles <= 3);
		}
	}

	#[test]
	fn test_partition_separates_disjoint_sources() {
		// Two groups with completely disjoint sources
		let groups = vec![
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 0, 0), tc(0, 1, 0), tc(0, 2, 0)],
			},
			SignatureGroup {
				sources: vec![2, 3],
				coords: vec![tc(1, 0, 0), tc(1, 1, 0), tc(1, 2, 0)],
			},
		];

		let batches = partition_into_batches(groups, 4, 3);
		// Each group has 3 tiles and batch_size=3, so they should land in separate batches
		assert_eq!(batches.len(), 2);

		for batch in &batches {
			let batch_tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert_eq!(batch_tiles, 3);
			// Each batch should only use sources from one group
			let all_sources: BTreeSet<usize> = batch.iter().flat_map(|g| g.sources.iter().copied()).collect();
			assert!(
				all_sources == [0, 1].into_iter().collect::<BTreeSet<_>>()
					|| all_sources == [2, 3].into_iter().collect::<BTreeSet<_>>()
			);
		}
	}

	#[test]
	fn test_partition_merges_identical_sources() {
		// Multiple groups with the same source set should stay together
		let groups = vec![
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 0, 0)],
			},
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 1, 0)],
			},
			SignatureGroup {
				sources: vec![0, 1],
				coords: vec![tc(0, 2, 0)],
			},
		];

		let batches = partition_into_batches(groups, 4, 10);
		// 3 tiles total, batch_size=10, should be one batch
		assert_eq!(batches.len(), 1);
		let total: usize = batches[0].iter().map(|g| g.coords.len()).sum();
		assert_eq!(total, 3);
	}

	#[test]
	fn test_partition_respects_batch_size() {
		let groups = vec![
			SignatureGroup {
				sources: vec![0],
				coords: (0..50).map(|i| tc(0, i, 0)).collect(),
			},
			SignatureGroup {
				sources: vec![1],
				coords: (0..50).map(|i| tc(1, i, 0)).collect(),
			},
			SignatureGroup {
				sources: vec![2],
				coords: (0..50).map(|i| tc(2, i, 0)).collect(),
			},
		];

		let batches = partition_into_batches(groups, 3, 40);
		// 150 tiles total, batch_size=40 → should have at least 4 batches
		assert!(batches.len() >= 4);

		for batch in &batches {
			let batch_tiles: usize = batch.iter().map(|g| g.coords.len()).sum();
			assert!(batch_tiles <= 40, "batch has {} tiles, exceeds limit of 40", batch_tiles);
		}

		// All tiles present
		let total: usize = batches.iter().flat_map(|b| b.iter()).map(|g| g.coords.len()).sum();
		assert_eq!(total, 150);
	}
}
