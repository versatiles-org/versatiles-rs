//! Assembly pipeline: tile processing, compositing, and output.
//!
//! The main entry point is [`assemble_tiles`], which runs the outer pass loop:
//!
//! ```text
//! loop {
//!     for each source:
//!         stream tiles → composite with buffer → write opaque tiles
//!         sweep-flush tiles in completed columns
//!         evict northern tiles if buffer exceeds limit
//!     flush remaining buffer
//!     if no eviction happened → done
//!     else → prepare next pass for northern tiles, repeat
//! }
//! ```
//!
//! Tile compositing happens in [`process_source_tiles`] via parallel `spawn_blocking`
//! tasks. Each tile follows one of these paths:
//!
//! 1. **Opaque, no overlap** — written as-is (original encoding preserved).
//! 2. **Opaque after merge** — composited result is opaque, re-encoded as lossy WebP.
//! 3. **Still translucent** — stays in the buffer for further compositing.
//! 4. **Translucent, never overlapped** — flushed at end of pass, re-encoded as WebP.

use super::pass_state::PassState;
use super::translucent_buffer::TranslucentBuffer;
use anyhow::{Context, Result, anyhow, ensure};
use futures::{StreamExt, future::ready};
use std::collections::HashMap;
use std::sync::Arc;
use versatiles_container::{Tile, TileSink, TilesRuntime, open_tile_sink};
use versatiles_core::{
	Blob, ConcurrencyLimits, MAX_ZOOM_LEVEL, TileBBoxPyramid, TileCompression, TileCoord, TileFormat, TileJSON,
	TileStream, utils::HilbertIndex,
};
use versatiles_image::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};

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

const NUM_LEVELS: usize = (MAX_ZOOM_LEVEL + 1) as usize;

type SuffixMinX = Vec<[Option<u32>; NUM_LEVELS]>;

/// Scan all sources in parallel, returning their pyramids in source order.
///
/// Limits concurrency to avoid exhausting file descriptors on systems with low `ulimit -n`.
pub async fn prescan_sources(paths: &[String], runtime: &TilesRuntime) -> Result<Vec<TileBBoxPyramid>> {
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

/// Build source processing order (west-to-east) and per-level suffix minimum x arrays.
///
/// Returns `(order, suffix)` where:
/// - `order[i]` is the index into `paths` of the i-th source to process.
/// - `suffix[i][level]` is the minimum x coordinate across all sources at positions `i..`
///   at the given zoom level. Used by [`sweep_flush`] to determine which columns are complete.
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

/// Compute the normalized western edge of a pyramid as the minimum fractional x across all levels.
fn western_edge(pyramid: &TileBBoxPyramid) -> f64 {
	pyramid.weighted_bbox().unwrap().x_min
}

/// Flush translucent tiles whose x-column is no longer covered by any remaining source.
///
/// After processing source at position `pos`, `remaining_min_x[level]` holds the minimum
/// x coordinate of all sources still to come at that zoom level. Any buffered tile whose
/// x < remaining_min_x is guaranteed to never be overlaid again, so it can be written now.
/// This is the key optimization of the sweep-line approach: it bounds memory by flushing
/// tiles as soon as they are complete, rather than waiting until all sources are processed.
fn sweep_flush(
	remaining_min_x: &[Option<u32>; NUM_LEVELS],
	buffer: &Arc<TranslucentBuffer>,
	sink: &Arc<Box<dyn TileSink>>,
	config: &AssembleConfig,
) -> Result<()> {
	log::debug!(
		"sweep-line flush: remaining_min_x={:?}",
		remaining_min_x
			.map(|x| x.map_or_else(|| "-".to_string(), |v| v.to_string()))
			.join(", ")
	);

	let tiles = buffer.remove_tiles_where(|coord| match remaining_min_x[coord.level as usize] {
		Some(min_x) => coord.x < min_x,
		None => true,
	});

	if tiles.is_empty() {
		return Ok(());
	}

	log::debug!("sweep-line flush: writing {} translucent tiles", tiles.len());

	let prepared = reencode_tiles_parallel(tiles, config);
	for result in prepared {
		let (coord, blob) = result?;
		sink.write_tile(&coord, &blob)?;
	}
	Ok(())
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
#[allow(clippy::too_many_arguments)]
pub async fn assemble_tiles(
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
	let buffer = Arc::new(TranslucentBuffer::new());

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

	let mut pass_state =
		prescanned_pyramids.map(|pyramids| PassState::new(pyramids, min_zoom, max_zoom, max_buffer_size, tile_dim));

	let sink: Arc<Box<dyn TileSink>> = Arc::new(open_tile_sink(
		output,
		config.tile_format,
		config.tile_compression,
		runtime,
	)?);

	loop {
		if let Some(ref mut ps) = pass_state {
			ps.start_pass();
		}
		buffer.clear();

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
			if let Some(ref ps) = pass_state {
				ps.clip_source_pyramid(&mut pyramid);
			}
			if pyramid.is_empty() {
				progress.inc(1);
				continue;
			}

			process_source_tiles(&reader, &pyramid, &sink, &buffer, &config).await?;

			if let Some(ref suffix) = suffix_min_x {
				sweep_flush(&suffix[pos + 1], &buffer, &sink, &config)?;
			}

			if let Some(ref mut ps) = pass_state {
				ps.check_eviction(&buffer);
			}

			progress.inc(1);
		}
		progress.finish();

		flush_translucent_tiles(&sink, buffer.drain(), &config, runtime)?;

		match pass_state {
			Some(ref mut ps) if !ps.is_pass_complete() => ps.prepare_next_pass(),
			_ => break,
		}
	}

	let sink = Arc::try_unwrap(sink).map_err(|_| anyhow!("sink still has references"))?;
	sink.finish(&tilejson, runtime)?;

	log::info!("finished mosaic assemble");
	Ok(())
}

/// Process all tiles from all levels of a source reader in a single parallel batch.
///
/// Flattens all level streams into one combined stream and uses
/// `cpu_count * 2` concurrency to keep CPUs saturated when task durations vary.
async fn process_source_tiles(
	reader: &versatiles_container::SharedTileSource,
	pyramid: &TileBBoxPyramid,
	sink: &Arc<Box<dyn TileSink>>,
	buffer: &Arc<TranslucentBuffer>,
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
	let buffer = Arc::clone(buffer);
	let config = config.clone();

	let callback = Arc::new(move |coord: TileCoord, mut tile: Tile| -> Result<()> {
		let key = coord.get_hilbert_index()?;

		let existing = buffer.remove(key);

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
						buffer.insert(coord, merged)?;
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
			buffer.insert(coord, tile)?;
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

	let prepared = reencode_tiles_parallel(tiles, config);
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
