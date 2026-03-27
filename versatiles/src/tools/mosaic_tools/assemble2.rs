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

use super::assemble::translucent_buffer::TranslucentBuffer;
use anyhow::{Context, Result, anyhow, ensure};
use futures::{StreamExt, future::ready};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use versatiles_container::{Tile, TileSink, TilesRuntime, open_tile_sink};
use versatiles_core::{
	Blob, ConcurrencyLimits, TileBBoxPyramid, TileCompression, TileCoord, TileFormat, TileJSON, TileStream,
	utils::HilbertIndex,
};
use versatiles_image::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};

/// CLI arguments for `mosaic assemble2`.
#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Assemble2 {
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

/// Encoding configuration shared across assemble2 functions.
#[derive(Clone)]
struct AssembleConfig {
	quality: [Option<u8>; 32],
	lossless: bool,
	tile_format: TileFormat,
	tile_compression: TileCompression,
}

// ─── CLI parsing helpers (duplicated from assemble to keep modules independent) ───

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

// ─── Tile compositing helpers (duplicated from assemble::pipeline) ───

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

fn prepare_tile_blob(tile: Tile, config: &AssembleConfig) -> Result<Blob> {
	tile.into_blob(config.tile_compression)
}

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

// ─── Entry point ───

pub async fn run(args: &Assemble2, runtime: &TilesRuntime) -> Result<()> {
	log::info!("mosaic assemble2 from {:?} to {:?}", args.input_list, args.output);

	let list_content = std::fs::read_to_string(&args.input_list)
		.with_context(|| format!("Failed to read input list file: {}", args.input_list))?;
	let paths = parse_input_list(&list_content);
	ensure!(!paths.is_empty(), "Input list file contains no container paths");

	let quality = parse_quality(&args.quality)?;
	let max_buffer_size = parse_buffer_size(&args.max_buffer_size)?;

	log::info!("assembling {} containers (two-pass)", paths.len());

	// Prescan all sources to get pyramids
	let mut pyramids = prescan_sources(&paths, runtime).await?;

	// Clip pyramids to requested zoom range
	for p in &mut pyramids {
		if let Some(min) = args.min_zoom {
			p.set_level_min(min);
		}
		if let Some(max) = args.max_zoom {
			p.set_level_max(max);
		}
	}

	assemble_two_pass(
		&args.output,
		&paths,
		&pyramids,
		&quality,
		args.lossless,
		max_buffer_size,
		runtime,
	)
	.await
}

// ─── Prescan ───

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

	let mut pyramids = vec![TileBBoxPyramid::default(); paths.len()];
	for result in results {
		let (idx, pyramid) = result?;
		pyramids[idx] = pyramid;
	}
	Ok(pyramids)
}

// ─── Two-pass pipeline ───

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn assemble_two_pass(
	output: &str,
	paths: &[String],
	pyramids: &[TileBBoxPyramid],
	quality: &[Option<u8>; 32],
	lossless: bool,
	max_buffer_size: u64,
	runtime: &TilesRuntime,
) -> Result<()> {
	// Discover config from first source
	let first_reader = runtime
		.get_reader_from_str(&paths[0])
		.await
		.with_context(|| format!("Failed to open container: {}", paths[0]))?;
	let first_metadata = first_reader.metadata();
	let first_tilejson = first_reader.tilejson();

	let config = AssembleConfig {
		quality: *quality,
		lossless,
		tile_format: first_metadata.tile_format,
		tile_compression: first_metadata.tile_compression,
	};

	let tile_dim: u64 = first_tilejson.tile_size.map_or(256, |ts| u64::from(ts.size()));

	let mut tilejson = TileJSON {
		tile_format: Some(config.tile_format),
		tile_type: Some(config.tile_format.to_type()),
		tile_schema: first_tilejson.tile_schema,
		tile_size: first_tilejson.tile_size,
		..TileJSON::default()
	};

	let sink: Arc<Box<dyn TileSink>> = Arc::new(open_tile_sink(
		output,
		config.tile_format,
		config.tile_compression,
		runtime,
	)?);

	// ─── First pass: scan + write opaque ───

	let mut translucent_map: HashMap<TileCoord, Vec<usize>> = HashMap::new();
	let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();

	let progress = runtime.create_progress("scanning sources", paths.len() as u64);

	for (idx, path) in paths.iter().enumerate() {
		let reader = runtime
			.get_reader_from_str(path)
			.await
			.with_context(|| format!("Failed to open container: {path}"))?;

		let metadata = reader.metadata();
		validate_source_format(path, metadata, &config)?;
		tilejson.merge(reader.tilejson())?;

		let pyramid = &pyramids[idx];
		if pyramid.is_empty() {
			progress.inc(1);
			continue;
		}

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
		let sink_ref = Arc::clone(&sink);
		let done_ref = Arc::clone(&done);
		let config_clone = config.clone();
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
				let blob = prepare_tile_blob(tile, &config_clone)?;
				sink_ref.write_tile(&coord, &blob)?;
				done_ref.lock().unwrap().insert(coord);
			} else {
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

	// ─── Between passes: prepare batches ───

	// Remove coords already written as opaque
	{
		let done_set = done.lock().unwrap();
		translucent_map.retain(|coord, _| !done_set.contains(coord));
	}

	if translucent_map.is_empty() {
		// All tiles were opaque — we're done
		let sink = Arc::try_unwrap(sink).map_err(|_| anyhow!("sink still has references"))?;
		sink.finish(&tilejson, runtime)?;
		log::info!("finished mosaic assemble2 (all tiles opaque, no second pass needed)");
		return Ok(());
	}

	// Sort by (level, x, y)
	let mut tile_list: Vec<(TileCoord, Vec<usize>)> = translucent_map.into_iter().collect();
	tile_list.sort_by(|a, b| {
		a.0.level
			.cmp(&b.0.level)
			.then(a.0.x.cmp(&b.0.x))
			.then(a.0.y.cmp(&b.0.y))
	});

	// Split into batches
	let batch_size = if max_buffer_size > 0 {
		usize::try_from((max_buffer_size / (tile_dim * tile_dim * 4)).max(1)).unwrap_or(usize::MAX)
	} else {
		tile_list.len()
	};
	let batches: Vec<&[(TileCoord, Vec<usize>)]> = tile_list.chunks(batch_size).collect();

	// ─── Second pass: composite translucent batches ───

	let total_source_reads: u64 = batches
		.iter()
		.map(|batch| {
			let sources: BTreeSet<usize> = batch.iter().flat_map(|(_, srcs)| srcs).copied().collect();
			sources.len() as u64
		})
		.sum();

	let progress = runtime.create_progress("compositing tiles", total_source_reads);

	for batch in &batches {
		let sources_needed: BTreeSet<usize> = batch.iter().flat_map(|(_, srcs)| srcs).copied().collect();

		let buffer = TranslucentBuffer::new();

		for &source_idx in &sources_needed {
			let path = &paths[source_idx];
			let reader = runtime
				.get_reader_from_str(path)
				.await
				.with_context(|| format!("Failed to open container: {path}"))?;

			// For each coord in the batch that this source covers, get the tile
			for &(coord, ref srcs) in *batch {
				if !srcs.contains(&source_idx) {
					continue;
				}

				if let Some(mut tile) = reader.get_tile(&coord).await? {
					if tile.is_empty()? {
						continue;
					}

					// Composite with buffer
					let key = coord.get_hilbert_index()?;
					let existing = buffer.remove(key);
					if let Some((_, existing_tile)) = existing {
						let (merged, _is_opaque) = merge_two_tiles(
							existing_tile,
							tile,
							config.quality[coord.level as usize],
							config.lossless,
						)?;
						buffer.insert(coord, merged)?;
					} else {
						buffer.insert(coord, tile)?;
					}
				}
			}

			progress.inc(1);
		}

		// Flush buffer to sink
		let tiles: Vec<_> = buffer.drain().into_values().collect();
		if !tiles.is_empty() {
			let prepared = reencode_tiles_parallel(tiles, &config);
			for result in prepared {
				let (coord, blob) = result?;
				sink.write_tile(&coord, &blob)?;
			}
		}
	}

	progress.finish();

	let sink = Arc::try_unwrap(sink).map_err(|_| anyhow!("sink still has references"))?;
	sink.finish(&tilejson, runtime)?;

	log::info!("finished mosaic assemble2");
	Ok(())
}
