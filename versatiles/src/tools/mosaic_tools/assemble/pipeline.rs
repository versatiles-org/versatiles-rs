//! Two-pass pipeline: scan sources → write opaque → batch-composite translucent.

use super::AssembleConfig;
use super::partitioning::{collapse_into_signature_groups, partition_into_batches};
use super::tiles::{
	composite_two_tiles, encode_tiles_parallel, fetch_source_tiles, validate_source_format, write_opaque_blob,
};
use super::translucent_buffer::TranslucentBuffer;
use anyhow::{Context, Result, anyhow};
use futures::{StreamExt, future::ready};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use versatiles_container::{Tile, TileSink, TilesRuntime, open_tile_sink};
use versatiles_core::{ConcurrencyLimits, TileCoord, TileJSON, TileStream, utils::HilbertIndex};

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn assemble_two_pass(
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
		log::debug!("finished mosaic assemble (all tiles opaque, no second pass needed)");
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
	log::debug!(
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
