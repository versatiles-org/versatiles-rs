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

/// A batch of tiles, each annotated with the source indices that contribute to it.
type TileBatch = Vec<(TileCoord, Vec<usize>)>;

/// Results collected during the first pass (source scanning).
pub(super) struct FirstPassResult {
	pub(super) config: AssembleConfig,
	pub(super) tilejson: TileJSON,
	pub(super) tile_dim: u64,
	pub(super) sink: Arc<Box<dyn TileSink>>,
	pub(super) translucent_map: HashMap<TileCoord, Vec<usize>>,
	pub(super) done: HashSet<TileCoord>,
}

// ─── First pass: scan sources + write opaque tiles ───

/// Stream every source once: write opaque tiles directly, record translucent coords.
#[allow(clippy::too_many_arguments)]
pub(super) async fn scan_sources(
	output: &str,
	paths: &[String],
	quality: &[Option<u8>; 32],
	lossless: bool,
	min_zoom: Option<u8>,
	max_zoom: Option<u8>,
	runtime: &TilesRuntime,
) -> Result<FirstPassResult> {
	let mut config: Option<AssembleConfig> = None;
	let mut tilejson: Option<TileJSON> = None;
	let mut tile_dim: u64 = 256;
	let mut sink: Option<Arc<Box<dyn TileSink>>> = None;

	let mut translucent_map: HashMap<TileCoord, Vec<usize>> = HashMap::new();
	let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();

	let progress = runtime.create_progress("pass 1/2: scanning sources", paths.len() as u64);

	for (idx, path) in paths.iter().enumerate() {
		let reader = runtime
			.reader_from_str(path)
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
		let level_bboxes: Vec<_> = pyramid.iter_bboxes().collect();
		let streams: Vec<TileStream<'static, Tile>> =
			futures::future::try_join_all(level_bboxes.into_iter().map(|bbox| reader.tile_stream(bbox))).await?;
		let combined = streams
			.into_iter()
			.reduce(TileStream::chain)
			.unwrap_or_else(TileStream::empty);

		let new_translucent = classify_and_stream_tiles(combined, cfg, Arc::clone(sink_arc), Arc::clone(&done)).await?;
		for coord in new_translucent {
			translucent_map.entry(coord).or_default().push(idx);
		}

		progress.inc(1);
	}

	progress.finish();

	let config = config.ok_or_else(|| anyhow!("no sources found"))?;
	let done = Arc::try_unwrap(done).map_err(|_| anyhow!("done set still has references"))?;

	Ok(FirstPassResult {
		config,
		tilejson: tilejson.unwrap(),
		tile_dim,
		sink: sink.unwrap(),
		translucent_map,
		done: done.into_inner().unwrap(),
	})
}

/// Classify tiles from a single source: write opaque, skip empty, collect translucent coords.
async fn classify_and_stream_tiles(
	stream: TileStream<'static, Tile>,
	config: AssembleConfig,
	sink: Arc<Box<dyn TileSink>>,
	done: Arc<Mutex<HashSet<TileCoord>>>,
) -> Result<Vec<TileCoord>> {
	let concurrency = ConcurrencyLimits::default().cpu_bound * 2;
	let translucent_coords: Arc<Mutex<Vec<TileCoord>>> = Arc::default();
	let translucent_coords_ref = Arc::clone(&translucent_coords);

	let callback = Arc::new(move |coord: TileCoord, mut tile: Tile| -> Result<()> {
		if done.lock().unwrap().contains(&coord) {
			return Ok(());
		}
		if tile.is_empty()? {
			return Ok(());
		}
		if tile.is_opaque()? {
			let blob = write_opaque_blob(tile, &config)?;
			sink.write_tile(&coord, &blob)?;
			done.lock().unwrap().insert(coord);
		} else {
			translucent_coords_ref.lock().unwrap().push(coord);
		}
		Ok(())
	});

	let mut result = Ok(());
	stream
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

	Ok(std::mem::take(&mut *translucent_coords.lock().unwrap()))
}

// ─── Between passes: prepare batches ───

/// Prune already-done coords, collapse into signature groups, partition into batches.
///
/// Returns `None` if all tiles were opaque (no second pass needed).
pub(super) fn prepare_batches(
	mut translucent_map: HashMap<TileCoord, Vec<usize>>,
	done: HashSet<TileCoord>,
	tile_dim: u64,
	max_buffer_size: u64,
	num_sources: usize,
) -> Option<Vec<TileBatch>> {
	translucent_map.retain(|coord, _| !done.contains(coord));
	drop(done);

	if translucent_map.is_empty() {
		return None;
	}

	let total_tiles = translucent_map.len();
	let groups = collapse_into_signature_groups(translucent_map);

	let batch_size = if max_buffer_size > 0 {
		usize::try_from((max_buffer_size / (tile_dim * tile_dim * 4)).max(1)).unwrap_or(usize::MAX)
	} else {
		total_tiles
	};

	let batch_groups = partition_into_batches(groups, num_sources, batch_size);

	let batches: Vec<TileBatch> = batch_groups
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

	Some(batches)
}

// ─── Second pass: composite translucent batches ───

/// For each batch, open only the needed sources, composite tiles, encode and flush.
pub(super) async fn composite_batches(
	batches: &[TileBatch],
	paths: &[String],
	config: &AssembleConfig,
	sink: &Arc<Box<dyn TileSink>>,
	runtime: &TilesRuntime,
) -> Result<()> {
	let total_source_reads: u64 = batches
		.iter()
		.map(|batch| {
			let sources: BTreeSet<usize> = batch.iter().flat_map(|(_, srcs)| srcs).copied().collect();
			sources.len() as u64
		})
		.sum();

	let progress = runtime.create_progress("pass 2/2: compositing tiles", total_source_reads);
	let concurrency = ConcurrencyLimits::default().cpu_bound;

	for batch in batches {
		composite_one_batch(batch, paths, config, sink, runtime, concurrency, &progress).await?;
	}

	progress.finish();
	Ok(())
}

/// Composite a single batch: fetch tiles from each needed source, composite, encode, flush.
async fn composite_one_batch(
	batch: &[(TileCoord, Vec<usize>)],
	paths: &[String],
	config: &AssembleConfig,
	sink: &Arc<Box<dyn TileSink>>,
	runtime: &TilesRuntime,
	concurrency: usize,
	progress: &versatiles_container::ProgressHandle,
) -> Result<()> {
	let sources_needed: Vec<usize> = {
		let set: BTreeSet<usize> = batch.iter().flat_map(|(_, srcs)| srcs).copied().collect();
		set.into_iter().collect()
	};

	let buffer = Arc::new(TranslucentBuffer::new());

	let mut prefetched: Option<Vec<(TileCoord, Tile)>> = None;

	let batch_arc = Arc::new(batch.to_vec());
	let paths_arc = Arc::new(paths.to_vec());

	for (pos, &source_idx) in sources_needed.iter().enumerate() {
		// Use pre-fetched tiles if available, otherwise fetch now
		let fetched_tiles = if let Some(tiles) = prefetched.take() {
			tiles
		} else {
			fetch_source_tiles(source_idx, batch, paths, runtime).await?
		};

		// Kick off pre-fetch for the NEXT source in the background
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
		let prepared = encode_tiles_parallel(tiles, config);
		for result in prepared {
			let (coord, blob) = result?;
			sink.write_tile(&coord, &blob)?;
		}
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord { level, x, y }
	}

	fn make_translucent_map(entries: &[(TileCoord, &[usize])]) -> HashMap<TileCoord, Vec<usize>> {
		entries.iter().map(|(c, s)| (*c, s.to_vec())).collect()
	}

	#[test]
	fn prepare_batches_returns_none_when_empty() {
		let map = HashMap::new();
		let done = HashSet::new();
		assert!(prepare_batches(map, done, 256, 1_000_000, 3).is_none());
	}

	#[test]
	fn prepare_batches_returns_none_when_all_done() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0, 1]), (tc(1, 0, 0), &[1, 2])]);
		let done: HashSet<TileCoord> = [tc(0, 0, 0), tc(1, 0, 0)].into_iter().collect();
		assert!(prepare_batches(map, done, 256, 1_000_000, 3).is_none());
	}

	#[test]
	fn prepare_batches_prunes_done_coords() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1]), (tc(2, 0, 0), &[2])]);
		let done: HashSet<TileCoord> = [tc(0, 0, 0)].into_iter().collect();
		let batches = prepare_batches(map, done, 256, 0, 3).unwrap();
		let total_tiles: usize = batches.iter().map(Vec::len).sum();
		assert_eq!(total_tiles, 2);
	}

	#[test]
	fn prepare_batches_unlimited_buffer_single_batch() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1]), (tc(2, 0, 0), &[2])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 0, 3).unwrap();
		// max_buffer_size=0 means unlimited → single batch
		assert_eq!(batches.len(), 1);
		assert_eq!(batches[0].len(), 3);
	}

	#[test]
	fn prepare_batches_respects_buffer_size() {
		// tile_dim=256, so each tile = 256*256*4 = 262144 bytes
		// max_buffer_size=300000 → batch_size=1
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1]), (tc(1, 1, 0), &[2])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 300_000, 3).unwrap();
		assert_eq!(batches.len(), 3);
		for batch in &batches {
			assert_eq!(batch.len(), 1);
		}
	}

	#[test]
	fn prepare_batches_preserves_source_indices() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0, 2]), (tc(1, 0, 0), &[1, 3])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 0, 4).unwrap();
		let all_tiles: Vec<_> = batches.into_iter().flatten().collect();
		assert_eq!(all_tiles.len(), 2);

		for (coord, sources) in &all_tiles {
			if *coord == tc(0, 0, 0) {
				assert_eq!(sources, &[0, 2]);
			} else if *coord == tc(1, 0, 0) {
				assert_eq!(sources, &[1, 3]);
			} else {
				panic!("unexpected coord: {coord:?}");
			}
		}
	}

	#[test]
	fn prepare_batches_all_tiles_present_after_batching() {
		let coords: Vec<TileCoord> = (0..20).map(|i| tc(0, i, 0)).collect();
		let mut map = HashMap::new();
		for (i, c) in coords.iter().enumerate() {
			map.insert(*c, vec![i % 3]);
		}
		// tile_dim=256, max_buffer_size=256*256*4*5 = 5 tiles per batch
		let max_buffer = 256 * 256 * 4 * 5;
		let batches = prepare_batches(map, HashSet::new(), 256, max_buffer, 3).unwrap();

		let total_tiles: usize = batches.iter().map(Vec::len).sum();
		assert_eq!(total_tiles, 20);

		for batch in &batches {
			assert!(batch.len() <= 5, "batch has {} tiles, expected <= 5", batch.len());
		}
	}

	#[test]
	fn prepare_batches_single_tile() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 0, 1).unwrap();
		assert_eq!(batches.len(), 1);
		assert_eq!(batches[0].len(), 1);
		assert_eq!(batches[0][0].0, tc(0, 0, 0));
		assert_eq!(batches[0][0].1, vec![0]);
	}

	#[test]
	fn prepare_batches_large_tile_dim() {
		// tile_dim=512, each tile = 512*512*4 = 1048576 bytes
		// max_buffer_size=2000000 → batch_size=1
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1])]);
		let batches = prepare_batches(map, HashSet::new(), 512, 2_000_000, 2).unwrap();
		assert_eq!(batches.len(), 2);
	}

	#[test]
	fn prepare_batches_small_tile_dim() {
		// tile_dim=64, each tile = 64*64*4 = 16384 bytes
		// max_buffer_size=50000 → batch_size=3
		let map = make_translucent_map(&[
			(tc(0, 0, 0), &[0]),
			(tc(1, 0, 0), &[1]),
			(tc(2, 0, 0), &[2]),
			(tc(3, 0, 0), &[3]),
			(tc(3, 1, 0), &[3]),
			(tc(3, 2, 0), &[3]),
			(tc(3, 3, 0), &[3]),
		]);
		let batches = prepare_batches(map, HashSet::new(), 64, 50_000, 4).unwrap();
		let total: usize = batches.iter().map(Vec::len).sum();
		assert_eq!(total, 7);
		for batch in &batches {
			assert!(batch.len() <= 3);
		}
	}

	#[test]
	fn prepare_batches_partial_done_overlap() {
		// Some coords in done, some not
		let map = make_translucent_map(&[
			(tc(0, 0, 0), &[0]),
			(tc(0, 1, 0), &[0]),
			(tc(1, 0, 0), &[1]),
			(tc(1, 1, 0), &[1]),
		]);
		let done: HashSet<TileCoord> = [tc(0, 0, 0), tc(1, 1, 0)].into_iter().collect();
		let batches = prepare_batches(map, done, 256, 0, 2).unwrap();
		let total: usize = batches.iter().map(Vec::len).sum();
		assert_eq!(total, 2);
	}

	#[test]
	fn prepare_batches_done_not_in_map_ignored() {
		// Done set has coords not in map — should not cause issues
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0])]);
		let done: HashSet<TileCoord> = [tc(5, 5, 5), tc(10, 10, 10)].into_iter().collect();
		let batches = prepare_batches(map, done, 256, 0, 1).unwrap();
		assert_eq!(batches.len(), 1);
		assert_eq!(batches[0].len(), 1);
	}

	#[test]
	fn prepare_batches_many_sources_many_tiles() {
		// Larger scenario: 100 tiles across 10 sources
		let mut map = HashMap::new();
		for i in 0..100u32 {
			let sources = vec![(i % 10) as usize, ((i + 1) % 10) as usize];
			map.insert(tc(0, i, 0), sources);
		}
		// batch_size fits ~25 tiles
		let max_buffer = 256 * 256 * 4 * 25;
		let batches = prepare_batches(map, HashSet::new(), 256, max_buffer, 10).unwrap();

		let total: usize = batches.iter().map(Vec::len).sum();
		assert_eq!(total, 100);

		for batch in &batches {
			assert!(batch.len() <= 25);
		}
	}

	#[test]
	fn prepare_batches_batch_size_equals_total() {
		// Exact boundary: batch_size == total_tiles
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(0, 1, 0), &[1]), (tc(0, 2, 0), &[2])]);
		// tile_dim=256, max_buffer_size = exactly 3 tiles
		let max_buffer = 256 * 256 * 4 * 3;
		let batches = prepare_batches(map, HashSet::new(), 256, max_buffer, 3).unwrap();
		assert_eq!(batches.len(), 1);
		assert_eq!(batches[0].len(), 3);
	}

	#[test]
	fn prepare_batches_sources_sorted_in_output() {
		// Source indices should remain sorted after collapse + partition
		let map = make_translucent_map(&[(tc(0, 0, 0), &[3, 1, 2])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 0, 5).unwrap();
		assert_eq!(batches[0][0].1, vec![1, 2, 3]); // sorted by collapse_into_signature_groups
	}

	#[test]
	fn prepare_batches_max_buffer_one_byte() {
		// Absurdly small buffer → batch_size=1 (clamped to at least 1)
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 1, 2).unwrap();
		// Each tile gets its own batch
		assert_eq!(batches.len(), 2);
	}
}
