use anyhow::{Context, Result, anyhow, ensure};
use futures::{StreamExt, future::ready};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use versatiles_container::{Tile, TileSink, TilesRuntime, open_tile_sink};
use versatiles_core::{
	Blob, ConcurrencyLimits, MAX_ZOOM_LEVEL, TileBBox, TileBBoxPyramid, TileCompression, TileCoord, TileFormat,
	TileJSON, TileStream, utils::HilbertIndex,
};
use versatiles_image::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};

/// Assemble multiple tile containers into a single output file.
///
/// Reads a list of tile containers (local paths or URLs), reads their tile indices,
/// and assembles them into a single output container. Handles overlapping tiles by compositing
/// semi-transparent images using additive alpha blending.
///
/// Tiles from containers listed earlier in the input file overlay tiles from containers listed later.
///
/// # Tile processing paths
///
/// Each tile coordinate follows one of these paths:
///
/// 1. **Opaque, no overlap** — The first tile seen at a coordinate is opaque.
///    Written as-is (original encoding preserved, no re-encoding).
///
/// 2. **Opaque after merge** — A translucent tile in the buffer is composited with a
///    new tile and the result is fully opaque. The merged image is re-encoded as lossy
///    WebP at the configured `--quality` and written.
///
/// 3. **Still translucent after merge** — Compositing produces a translucent result.
///    The merged image is re-encoded as WebP (lossy at `--quality`, or lossless if
///    `--lossless` is set) and kept in the buffer for further compositing.
///
/// 4. **Translucent, never overlapped** — A translucent tile that is never covered by
///    another source. At flush time it is re-encoded as lossy WebP at `--quality`
///    (or lossless WebP if `--lossless` is set) before writing.
///
/// In short: opaque tiles pass through unchanged, while every translucent tile that
/// reaches the output is re-encoded with the user's quality/lossless settings.
#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Assemble {
	/// Text file listing container paths or URLs, one per line.
	/// Empty lines and # comments are skipped. Whitespace is trimmed.
	/// Containers listed earlier overlay containers listed later.
	input_list: String,

	/// Output container path (currently supports .tar, directory, or .mbtiles).
	output: String,

	/// Lossy WebP quality for the final output tiles, using zoom-dependent syntax
	/// (e.g. "70,14:50,15:20"). Default: 75.
	#[arg(long, value_name = "str", default_value = "75")]
	quality: String,

	/// Encode translucent tiles as lossless WebP instead of using the lossy --quality setting
	#[arg(long)]
	lossless: bool,

	/// Allow reordering input files to optimize processing speed.
	/// Scans all sources upfront and sorts them west-to-east for efficient sweep-line flushing.
	#[arg(long)]
	optimize_order: bool,

	/// Minimum zoom level to include in the output (default: include all).
	#[arg(long, value_name = "int")]
	min_zoom: Option<u8>,

	/// Maximum zoom level to include in the output (default: include all).
	#[arg(long, value_name = "int")]
	max_zoom: Option<u8>,

	/// Maximum memory for the tile buffer.
	/// Supports units: k, m, g, t (e.g. "4g") and % of system memory (e.g. "50%").
	/// Plain number is interpreted as bytes. 0 means unlimited (default).
	#[arg(long, value_name = "size", default_value = "0")]
	max_buffer_size: String,
}

/// Encoding and filtering configuration shared across assemble functions.
#[derive(Clone)]
struct AssembleConfig {
	quality: [Option<u8>; 32],
	lossless: bool,
	tile_format: TileFormat,
	tile_compression: TileCompression,
	min_zoom: Option<u8>,
	max_zoom: Option<u8>,
}

/// Parse a quality string (e.g. "70,14:50,15:20") into per-zoom-level values.
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

/// Parse the input list file, returning a list of container paths/URLs.
fn parse_input_list(content: &str) -> Vec<String> {
	content
		.lines()
		.map(|line| {
			// Strip comments
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

/// Parse a buffer size string into bytes.
///
/// Accepts plain numbers (bytes), numbers with unit suffix (k, m, g, t),
/// or a percentage of total system memory (e.g. "50%").
/// Case-insensitive. Whitespace between number and unit is allowed.
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
	let (num_str, multiplier) = if let Some(n) = s_lower.strip_suffix('t') {
		(n, 1_000_000_000_000u64)
	} else if let Some(n) = s_lower.strip_suffix('g') {
		(n, 1_000_000_000)
	} else if let Some(n) = s_lower.strip_suffix('m') {
		(n, 1_000_000)
	} else if let Some(n) = s_lower.strip_suffix('k') {
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

/// Return total physical memory in bytes.
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

pub async fn run(args: &Assemble, runtime: &TilesRuntime) -> Result<()> {
	log::info!("mosaic assemble from {:?} to {:?}", args.input_list, args.output);

	let list_content = std::fs::read_to_string(&args.input_list)
		.with_context(|| format!("Failed to read input list file: {}", args.input_list))?;
	let paths = parse_input_list(&list_content);
	ensure!(!paths.is_empty(), "Input list file contains no container paths");

	let quality = parse_quality(&args.quality)?;
	let max_buffer_size = parse_buffer_size(&args.max_buffer_size)?;

	log::info!("assembling {} containers", paths.len());

	// Optionally prescan all sources in parallel to validate accessibility and collect pyramids
	let do_prescan = args.optimize_order || max_buffer_size > 0;
	let prescanned_pyramids = if do_prescan {
		Some(prescan_sources(&paths, runtime).await?)
	} else {
		None
	};

	assemble_tiles(
		&args.output,
		&paths,
		prescanned_pyramids.as_deref(),
		&quality,
		args.lossless,
		args.min_zoom,
		args.max_zoom,
		max_buffer_size,
		runtime,
	)
	.await
}

/// Scan all sources in parallel, returning their pyramids in source order.
///
/// Limits concurrency to avoid exhausting file descriptors on systems with low `ulimit -n`.
async fn prescan_sources(paths: &[String], runtime: &TilesRuntime) -> Result<Vec<TileBBoxPyramid>> {
	let progress = runtime.create_progress("scanning containers", paths.len() as u64);
	let concurrency = ConcurrencyLimits::default().io_bound;

	let results: Vec<Result<(usize, TileBBoxPyramid)>> = futures::stream::iter(paths.iter().enumerate())
		.map(|(idx, path)| {
			let runtime = runtime.clone();
			let path = path.clone();
			let progress = progress.clone();
			async move {
				let reader = runtime
					.get_reader_from_str(&path)
					.await
					.with_context(|| format!("Failed to open container: {path}"))?;
				let pyramid = reader.metadata().bbox_pyramid.clone();
				progress.inc(1);
				Ok((idx, pyramid))
			}
		})
		.buffer_unordered(concurrency)
		.collect()
		.await;

	progress.finish();

	// Restore original order
	let mut pyramids = vec![TileBBoxPyramid::default(); paths.len()];
	for result in results {
		let (idx, pyramid) = result?;
		pyramids[idx] = pyramid;
	}
	Ok(pyramids)
}

const NUM_LEVELS: usize = (MAX_ZOOM_LEVEL + 1) as usize;

type SuffixMinX = Vec<[Option<u32>; NUM_LEVELS]>;

/// Build source processing order (west-to-east) and per-level suffix minimum x arrays.
fn build_sweep_order(num_sources: usize, pyramids: &[TileBBoxPyramid]) -> (Vec<usize>, SuffixMinX) {
	let mut order: Vec<usize> = (0..num_sources).collect();
	order.sort_unstable_by(|&a, &b| western_edge(&pyramids[a]).total_cmp(&western_edge(&pyramids[b])));

	let mut suffix: SuffixMinX = vec![[None; NUM_LEVELS]; order.len() + 1];
	for pos in (0..order.len()).rev() {
		let idx = order[pos];
		suffix[pos] = suffix[pos + 1];
		for level in 0..=(MAX_ZOOM_LEVEL) {
			let bbox = pyramids[idx].get_level_bbox(level);
			if !bbox.is_empty()
				&& let Ok(x) = bbox.x_min()
			{
				let l = level as usize;
				suffix[pos][l] = Some(match suffix[pos][l] {
					Some(existing) => existing.min(x),
					None => x,
				});
			}
		}
	}
	(order, suffix)
}

/// Flush translucent tiles whose x-column is no longer covered by any remaining source.
fn sweep_flush(
	remaining_min_x: &[Option<u32>; NUM_LEVELS],
	translucent_buffer: &Arc<Mutex<HashMap<u64, (TileCoord, Tile)>>>,
	sink: &Arc<Box<dyn TileSink>>,
	config: &AssembleConfig,
) -> Result<()> {
	log::debug!(
		"sweep-line flush: remaining_min_x={:?}",
		remaining_min_x
			.map(|x| x.map_or_else(|| "-".to_string(), |v| v.to_string()))
			.join(", ")
	);

	let mut buf = translucent_buffer.lock().unwrap();
	let flush_keys: Vec<u64> = buf
		.iter()
		.filter(|(_, (coord, _))| match remaining_min_x[coord.level as usize] {
			Some(min_x) => coord.x < min_x,
			None => true,
		})
		.map(|(&key, _)| key)
		.collect();

	if flush_keys.is_empty() {
		return Ok(());
	}

	let tiles: Vec<_> = flush_keys.iter().filter_map(|k| buf.remove(k)).collect();
	drop(buf);

	log::debug!("sweep-line flush: writing {} translucent tiles", tiles.len());

	// Phase 1: Re-encode in parallel
	let prepared = reencode_tiles_parallel(tiles, config);

	// Phase 2: Write sequentially
	for result in prepared {
		let (coord, blob) = result?;
		sink.write_tile(&coord, &blob)?;
	}
	Ok(())
}

/// Compute the normalized western edge of a pyramid as the minimum fractional x across all levels.
fn western_edge(pyramid: &TileBBoxPyramid) -> f64 {
	pyramid.weighted_bbox().unwrap().x_min
}

/// Open the first source to discover tile format, compression, and metadata.
/// Returns the config and tilejson fields to populate the output.
async fn discover_config(
	paths: &[String],
	source_order: &[usize],
	quality: &[Option<u8>; 32],
	lossless: bool,
	min_zoom: Option<u8>,
	max_zoom: Option<u8>,
	runtime: &TilesRuntime,
) -> Result<(AssembleConfig, TileJSON, u64)> {
	let first_idx = source_order[0];
	let first_path = &paths[first_idx];
	let first_reader = runtime
		.get_reader_from_str(first_path)
		.await
		.with_context(|| format!("Failed to open container: {first_path}"))?;
	let first_metadata = first_reader.metadata();
	let first_tilejson = first_reader.tilejson();

	let config = AssembleConfig {
		quality: *quality,
		lossless,
		tile_format: first_metadata.tile_format,
		tile_compression: first_metadata.tile_compression,
		min_zoom,
		max_zoom,
	};

	let tile_dim: u64 = first_tilejson.tile_size.map_or(256, |ts| u64::from(ts.size()));

	let tilejson = TileJSON {
		tile_format: Some(config.tile_format),
		tile_type: Some(config.tile_format.to_type()),
		tile_schema: first_tilejson.tile_schema,
		tile_size: first_tilejson.tile_size,
		..TileJSON::default()
	};

	Ok((config, tilejson, tile_dim))
}

/// Build a union pyramid from all prescanned pyramids, clipped to zoom range.
fn build_union_pyramid(pyramids: &[TileBBoxPyramid], min_zoom: Option<u8>, max_zoom: Option<u8>) -> TileBBoxPyramid {
	let mut u = TileBBoxPyramid::new_empty();
	for p in pyramids {
		u.include_pyramid(p);
	}
	if let Some(min) = min_zoom {
		u.set_level_min(min);
	}
	if let Some(max) = max_zoom {
		u.set_level_max(max);
	}
	u
}

/// Evict northern tiles from the buffer and clip the remaining pyramid.
///
/// Tiles at levels where `level_cut_y == 0` are not evicted because they
/// cannot be reprocessed in a subsequent pass.
fn evict_northern_tiles(
	buffer: &mut HashMap<u64, (TileCoord, Tile)>,
	remaining_pyramid: &mut TileBBoxPyramid,
	cut_y: u32,
	max_level: u8,
) {
	// Remove tiles north of the cut from buffer
	let evict_keys: Vec<u64> = buffer
		.iter()
		.filter(|(_, (coord, _))| {
			let level_cut_y = cut_y >> (max_level - coord.level);
			level_cut_y > 0 && coord.y < level_cut_y
		})
		.map(|(&k, _)| k)
		.collect();
	for k in &evict_keys {
		buffer.remove(k);
	}
	log::debug!(
		"evicted {} tiles, {} remaining in buffer",
		evict_keys.len(),
		buffer.len()
	);

	// Clip remaining_pyramid so subsequent sources skip evicted region
	for level in 0..=max_level {
		let level_cut_y = cut_y >> (max_level - level);
		let bbox = remaining_pyramid.get_level_bbox(level);
		if !bbox.is_empty()
			&& let Ok(y_min) = bbox.y_min()
			&& y_min < level_cut_y
		{
			let mut new_bbox = *bbox;
			let _ = new_bbox.set_y_min(level_cut_y);
			remaining_pyramid.set_level_bbox(new_bbox);
		}
	}
}

/// Create a pyramid covering only tiles north of `cut_y` (for the next pass).
fn clip_pyramid_to_north(union_pyramid: &TileBBoxPyramid, cut_y: u32, max_level: u8) -> TileBBoxPyramid {
	let mut next = union_pyramid.clone();
	for level in 0..=max_level {
		let shift = max_level - level;
		let level_cut_y = cut_y >> shift;
		let bbox = next.get_level_bbox(level);
		if !bbox.is_empty() {
			if level_cut_y == 0 {
				next.set_level_bbox(TileBBox::new_empty(level).unwrap());
			} else {
				let mut new_bbox = *bbox;
				let _ = new_bbox.set_y_max(level_cut_y - 1);
				next.set_level_bbox(new_bbox);
			}
		}
	}
	next
}

/// Validate that a source's format and compression match the expected config.
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

/// Assemble sources into an output container. If `prescanned_pyramids` is provided, uses those
/// instead of reading pyramids from each source during assembly.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn assemble_tiles(
	output: &str,
	paths: &[String],
	prescanned_pyramids: Option<&[TileBBoxPyramid]>,
	quality: &[Option<u8>; 32],
	lossless: bool,
	min_zoom: Option<u8>,
	max_zoom: Option<u8>,
	max_buffer_size: u64,
	runtime: &TilesRuntime,
) -> Result<()> {
	let translucent_buffer: Arc<Mutex<HashMap<u64, (TileCoord, Tile)>>> = Arc::default();

	// When prescan data is available, sort sources west-to-east and precompute
	// per-level suffix minimum x so we can flush translucent tiles early.
	let (source_order, suffix_min_x): (Vec<usize>, Option<SuffixMinX>) = if let Some(pyramids) = prescanned_pyramids {
		let (order, suffix) = build_sweep_order(paths.len(), pyramids);
		log::debug!(
			"source processing order (west to east): {:?}",
			order.iter().map(|&i| &paths[i]).collect::<Vec<_>>()
		);
		(order, Some(suffix))
	} else {
		((0..paths.len()).collect(), None)
	};

	let (config, mut tilejson, tile_dim) =
		discover_config(paths, &source_order, quality, lossless, min_zoom, max_zoom, runtime).await?;

	let max_buffer_tiles: usize = if max_buffer_size > 0 {
		usize::try_from(max_buffer_size / (tile_dim * tile_dim * 4)).unwrap_or(usize::MAX)
	} else {
		usize::MAX
	};

	let (union_pyramid, max_level): (Option<TileBBoxPyramid>, u8) = if let Some(pyramids) = prescanned_pyramids {
		let u = build_union_pyramid(pyramids, min_zoom, max_zoom);
		let ml = u.get_level_max().unwrap_or(0);
		(Some(u), ml)
	} else {
		(None, 0)
	};

	let sink: Arc<Box<dyn TileSink>> = Arc::new(open_tile_sink(
		output,
		config.tile_format,
		config.tile_compression,
		runtime,
	)?);

	// Outer pass loop: each pass processes tiles south of cut_y.
	// When buffer exceeds max_buffer_tiles, northern tiles are evicted
	// and deferred to subsequent passes.
	let mut remaining_pyramid = union_pyramid.clone();

	loop {
		let mut cut_y: u32 = 0;
		translucent_buffer.lock().unwrap().clear();

		let progress = runtime.create_progress("assembling tiles", paths.len() as u64);

		for (pos, &idx) in source_order.iter().enumerate() {
			let path = &paths[idx];
			let reader = runtime
				.get_reader_from_str(path)
				.await
				.with_context(|| format!("Failed to open container: {path}"))?;

			log::debug!("processing source {}/{}: {path}", pos + 1, source_order.len());

			let metadata = reader.metadata();
			validate_source_format(path, metadata, &config)?;
			tilejson.merge(reader.tilejson())?;

			let mut pyramid = prescanned_pyramids.map_or(&metadata.bbox_pyramid, |p| &p[idx]).clone();
			if let Some(min) = config.min_zoom {
				pyramid.set_level_min(min);
			}
			if let Some(max) = config.max_zoom {
				pyramid.set_level_max(max);
			}
			if let Some(ref rp) = remaining_pyramid {
				pyramid.intersect(rp);
			}
			if pyramid.is_empty() {
				progress.inc(1);
				continue;
			}

			process_source_tiles(&reader, &pyramid, &sink, &translucent_buffer, &config).await?;

			if let Some(ref suffix) = suffix_min_x {
				sweep_flush(&suffix[pos + 1], &translucent_buffer, &sink, &config)?;
			}

			// Eviction: if buffer exceeds limit, evict northern tiles
			let buf_len = translucent_buffer.lock().unwrap().len();
			log::trace!(
				"after source {}/{}: buffer={buf_len} tiles",
				pos + 1,
				source_order.len()
			);
			if buf_len > max_buffer_tiles
				&& let Some(ref mut rp) = remaining_pyramid
			{
				let new_cut_y = compute_new_cut_y(&translucent_buffer.lock().unwrap(), max_buffer_tiles, max_level);
				if new_cut_y > cut_y {
					cut_y = new_cut_y;
					log::debug!(
						"buffer exceeded limit ({buf_len} > {max_buffer_tiles}), evicting tiles north of cut_y={cut_y}"
					);
					evict_northern_tiles(&mut translucent_buffer.lock().unwrap(), rp, cut_y, max_level);
				}
			}

			progress.inc(1);
		}
		progress.finish();

		// Flush remaining translucent tiles for this pass
		flush_translucent_tiles(
			&sink,
			translucent_buffer.lock().unwrap().drain().collect(),
			&config,
			runtime,
		)?;

		if cut_y == 0 {
			break;
		}

		if let Some(ref up) = union_pyramid {
			remaining_pyramid = Some(clip_pyramid_to_north(up, cut_y, max_level));
		}
		log::debug!("pass complete, restarting for remaining tiles above cut_y={cut_y}");
	}

	// Finalize: write metadata and close the container
	let sink = Arc::try_unwrap(sink).map_err(|_| anyhow!("sink still has references"))?;
	sink.finish(&tilejson, runtime)?;

	log::info!("finished mosaic assemble");
	Ok(())
}

/// Compute the new cut_y value to keep approximately `max_tiles` southern tiles in the buffer.
/// Returns the projected y coordinate (at max_level) below which tiles should be evicted.
fn compute_new_cut_y(buffer: &HashMap<u64, (TileCoord, Tile)>, max_tiles: usize, max_level: u8) -> u32 {
	let mut projected_ys: Vec<u32> = buffer
		.values()
		.map(|(coord, _)| coord.y << (max_level - coord.level))
		.collect();
	projected_ys.sort_unstable();
	// Keep max_tiles entries with the highest y values (southernmost)
	if projected_ys.len() > max_tiles {
		projected_ys[projected_ys.len() - max_tiles]
	} else {
		0
	}
}

/// Process all tiles from all levels of a source reader in a single parallel batch.
///
/// Flattens all level streams into one combined stream and uses
/// `cpu_count * 2` concurrency to keep CPUs saturated when task durations vary.
async fn process_source_tiles(
	reader: &versatiles_container::SharedTileSource,
	pyramid: &TileBBoxPyramid,
	sink: &Arc<Box<dyn TileSink>>,
	translucent_buffer: &Arc<Mutex<HashMap<u64, (TileCoord, Tile)>>>,
	config: &AssembleConfig,
) -> Result<()> {
	// Flatten all level streams into one combined stream
	let level_bboxes: Vec<_> = pyramid.iter_levels().filter(|b| !b.is_empty()).copied().collect();
	let streams: Vec<TileStream<'static, Tile>> =
		futures::future::try_join_all(level_bboxes.into_iter().map(|bbox| reader.get_tile_stream(bbox))).await?;
	let combined = streams
		.into_iter()
		.reduce(TileStream::chain)
		.unwrap_or_else(TileStream::empty);

	// Custom parallel loop with cpu_count * 2 concurrency
	let concurrency = ConcurrencyLimits::default().cpu_bound * 2;

	let sink = Arc::clone(sink);
	let translucent_buffer = Arc::clone(translucent_buffer);
	let config = config.clone();

	let callback = Arc::new(move |coord: TileCoord, mut tile: Tile| -> Result<()> {
		let key = coord.get_hilbert_index()?;

		let existing = translucent_buffer.lock().unwrap().remove(&key);

		if tile.is_empty()? {
			log::trace!("skipping empty tile at {coord:?}");
		}

		if let Some((_, existing)) = existing {
			match merge_two_tiles(tile, existing, config.quality[coord.level as usize], config.lossless) {
				Ok((merged, is_opaque)) => {
					if is_opaque {
						let blob = prepare_tile_blob(merged, &config)?;
						sink.write_tile(&coord, &blob)?;
					} else {
						translucent_buffer.lock().unwrap().insert(key, (coord, merged));
					}
				}
				Err(e) => {
					return Err(e.context(format!("Failed to merge tile at {coord:?}")));
				}
			}
		} else if tile.is_opaque().unwrap_or(false) {
			let blob = prepare_tile_blob(tile, &config)?;
			sink.write_tile(&coord, &blob)?;
		} else {
			translucent_buffer.lock().unwrap().insert(key, (coord, tile));
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
	result
}

/// Flush remaining translucent tiles to the output.
fn flush_translucent_tiles(
	sink: &Arc<Box<dyn TileSink>>,
	translucent_buffer: HashMap<u64, (TileCoord, Tile)>,
	config: &AssembleConfig,
	runtime: &TilesRuntime,
) -> Result<()> {
	if translucent_buffer.is_empty() {
		return Ok(());
	}
	let tiles: Vec<_> = translucent_buffer.into_values().collect();
	let progress = runtime.create_progress("flushing translucent tiles", tiles.len() as u64);

	// Phase 1: Re-encode in parallel
	let prepared = reencode_tiles_parallel(tiles, config);

	// Phase 2: Write sequentially
	for result in prepared {
		let (coord, blob) = result?;
		sink.write_tile(&coord, &blob)?;
		progress.inc(1);
	}
	progress.finish();
	Ok(())
}

/// Re-encode translucent tiles in parallel using scoped threads.
///
/// Returns a vec of prepared (coord, blob) results ready for sequential writing.
fn reencode_tiles_parallel(tiles: Vec<(TileCoord, Tile)>, config: &AssembleConfig) -> Vec<Result<(TileCoord, Blob)>> {
	let config = config.clone();
	std::thread::scope(|s| {
		let handles: Vec<_> = tiles
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
					let blob = prepare_tile_blob(tile, cfg)?;
					Ok((coord, blob))
				})
			})
			.collect();
		handles.into_iter().map(|h| h.join().unwrap()).collect()
	})
}

/// Compress a tile into a blob.
/// This is the CPU-heavy part — call without holding any locks.
fn prepare_tile_blob(tile: Tile, config: &AssembleConfig) -> Result<Blob> {
	tile.into_blob(config.tile_compression)
}

/// Merge two tiles: `base` (bottom) and `top` (overlay on top).
/// Returns the merged tile and whether it is opaque.
fn merge_two_tiles(base: Tile, mut top: Tile, quality: Option<u8>, lossless: bool) -> Result<(Tile, bool)> {
	if top.is_opaque()? {
		return Ok((top, true));
	}

	let base_image = base.into_image()?;
	let top_image = top.into_image()?;

	let mut result = base_image;
	result.overlay_additive(&top_image)?;

	let is_opaque = result.is_opaque();
	let effective_quality = if !is_opaque && lossless { Some(100) } else { quality };

	let mut tile = Tile::from_image(result, TileFormat::WEBP)?;
	tile.change_format(TileFormat::WEBP, effective_quality, None)?;
	Ok((tile, is_opaque))
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
		assert_eq!(parse_buffer_size("2m").unwrap(), 2_000_000);
		assert_eq!(parse_buffer_size("2M").unwrap(), 2_000_000);
		assert_eq!(parse_buffer_size("3g").unwrap(), 3_000_000_000);
		assert_eq!(parse_buffer_size("3G").unwrap(), 3_000_000_000);
		assert_eq!(parse_buffer_size("1t").unwrap(), 1_000_000_000_000);

		// Fractional with unit
		assert_eq!(parse_buffer_size("1.5g").unwrap(), 1_500_000_000);
		assert_eq!(parse_buffer_size("0.5m").unwrap(), 500_000);

		// Whitespace
		assert_eq!(parse_buffer_size("  4g  ").unwrap(), 4_000_000_000);
		assert_eq!(parse_buffer_size("2 m").unwrap(), 2_000_000);

		// Percentage
		let result = parse_buffer_size("50%").unwrap();
		assert!(result > 0, "50% of system memory should be > 0");

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
}
