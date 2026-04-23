//! Two-pass pipeline: scan sources → write opaque → batch-composite translucent.

use super::AssembleConfig;
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
///
/// Sources inside the batch are processed in ascending index order during the
/// second pass; tiles are flushed as soon as all of their sources have been
/// composited. `max_tiles_in_memory` reports the peak occupancy of the
/// compositing buffer under that schedule, so the packer can keep the peak
/// bounded by `max_buffer_size / tile_bytes`.
pub(super) struct TileBatch {
	/// Tuples of `(coord, sources)`. `sources` is sorted ascending and deduped.
	tiles: Vec<(TileCoord, Vec<usize>)>,
}

impl TileBatch {
	fn new() -> Self {
		Self { tiles: Vec::new() }
	}

	#[cfg(test)]
	pub(super) fn len(&self) -> usize {
		self.tiles.len()
	}

	pub(super) fn tiles(&self) -> &[(TileCoord, Vec<usize>)] {
		&self.tiles
	}

	fn push(&mut self, coord: TileCoord, sources: Vec<usize>) {
		self.tiles.push((coord, sources));
	}

	/// All unique source indices used in this batch, sorted ascending.
	pub(super) fn sources(&self) -> Vec<usize> {
		let set: BTreeSet<usize> = self.tiles.iter().flat_map(|(_, s)| s).copied().collect();
		set.into_iter().collect()
	}

	#[cfg(test)]
	pub(super) fn into_tiles(self) -> Vec<(TileCoord, Vec<usize>)> {
		self.tiles
	}

	/// Peak number of tiles simultaneously held in the compositing buffer when
	/// sources are processed in ascending index order and each tile is flushed
	/// as soon as its last contributing source has been composited.
	pub(super) fn max_tiles_in_memory(&self) -> usize {
		match self.tiles.len() {
			0 => return 0,
			1 => return 1,
			_ => {}
		}

		let sources = self.sources();
		let n = sources.len();
		// Split +1/-1 events into two unsigned vectors to avoid signed arithmetic:
		// `starts[i]` tiles come alive at step i; `ends[i]` tiles finish their last
		// source at step i and are flushed before step i+1.
		let mut starts = vec![0usize; n];
		let mut ends = vec![0usize; n];
		for (_, srcs) in &self.tiles {
			let positions = srcs
				.iter()
				.map(|s| sources.binary_search(s).expect("source present in batch"));
			let (first, last) = positions.fold((usize::MAX, 0usize), |(lo, hi), p| (lo.min(p), hi.max(p)));
			starts[first] += 1;
			ends[last] += 1;
		}

		let mut peak = 0usize;
		let mut running = 0usize;
		for i in 0..n {
			running += starts[i];
			if running > peak {
				peak = running;
			}
			running -= ends[i];
		}
		peak
	}
}

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
				tile_format: *metadata.tile_format(),
				tile_compression: *metadata.tile_compression(),
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

		tilejson
			.as_mut()
			.expect("tilejson initialized on first iteration")
			.merge(reader_tilejson)?;

		// Get pyramid and clip to zoom range
		let mut pyramid = reader.tile_pyramid().await?.as_ref().clone();
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

		let sink_arc = sink.as_ref().expect("sink initialized on first iteration");

		// Stream all tiles from this source
		let level_bboxes: Vec<_> = pyramid.to_iter_bboxes().collect();
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
		tilejson: tilejson.expect("tilejson initialized on first iteration"),
		tile_dim,
		sink: sink.expect("sink initialized on first iteration"),
		translucent_map,
		done: done.into_inner().expect("poisoned mutex"),
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
		if done.lock().expect("poisoned mutex").contains(&coord) {
			return Ok(());
		}
		if tile.is_empty()? {
			return Ok(());
		}
		if tile.is_opaque()? {
			let blob = write_opaque_blob(tile, &config)?;
			sink.write_tile(&coord, &blob)?;
			done.lock().expect("poisoned mutex").insert(coord);
		} else {
			translucent_coords_ref.lock().expect("poisoned mutex").push(coord);
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

	Ok(std::mem::take(&mut *translucent_coords.lock().expect("poisoned mutex")))
}

// ─── Between passes: prepare batches ───

/// Prune already-done coords and pack the remaining translucent tiles into batches.
///
/// The packer is memory-aware: it seeds each batch with the tile requiring the most
/// sources, then greedily adds more tiles that maximise source overlap — using the
/// batch's `max_tiles_in_memory` as its admission criterion. Because tiles flush as
/// soon as their last source has been composited, many batches can hold significantly
/// more tiles than the naive `max_buffer_size / tile_bytes` bound.
pub(super) fn prepare_batches(
	mut translucent_map: HashMap<TileCoord, Vec<usize>>,
	done: HashSet<TileCoord>,
	tile_dim: u64,
	max_buffer_size: u64,
	_num_sources: usize,
) -> Vec<TileBatch> {
	translucent_map.retain(|coord, _| !done.contains(coord));
	drop(done);

	if translucent_map.is_empty() {
		return vec![];
	}

	let total_tiles = translucent_map.len();

	// Each tile's source list must be sorted+deduped so `max_tiles_in_memory` and the
	// scoring step can treat it as a canonical signature.
	let mut pool: Vec<(TileCoord, Vec<usize>)> = translucent_map
		.into_iter()
		.map(|(c, mut s)| {
			s.sort_unstable();
			s.dedup();
			(c, s)
		})
		.collect();

	let batch_size: usize = if max_buffer_size > 0 {
		usize::try_from((max_buffer_size / (tile_dim * tile_dim * 4)).max(1)).unwrap_or(usize::MAX)
	} else {
		total_tiles
	};

	let mut batches: Vec<TileBatch> = Vec::new();

	while !pool.is_empty() {
		let mut batch = TileBatch::new();

		// Step 1: seed with the tile needing the most sources. Deterministic tiebreak
		// on (level, x, y) so equal-length pools yield stable output across runs.
		let seed_idx = pool
			.iter()
			.enumerate()
			.max_by(|(_, (ca, sa)), (_, (cb, sb))| {
				sa.len()
					.cmp(&sb.len())
					.then_with(|| cb.level.cmp(&ca.level))
					.then_with(|| cb.x.cmp(&ca.x))
					.then_with(|| cb.y.cmp(&ca.y))
			})
			.map(|(i, _)| i)
			.expect("pool not empty");
		let (seed_coord, seed_sources) = pool.swap_remove(seed_idx);
		batch.push(seed_coord, seed_sources);

		loop {
			// Step 2 + 3: peak occupancy → remaining room.
			let peak = batch.max_tiles_in_memory();
			let room = batch_size.saturating_sub(peak);

			// Step 4: batch is full.
			if room == 0 || pool.is_empty() {
				break;
			}

			// Step 5: score each remaining tile — reward overlap, penalise new sources.
			// The ranking key is `(overlap desc, new_sources asc)` which matches
			// "favour overlap, penalise new sources" without any signed arithmetic.
			let batch_sources: BTreeSet<usize> = batch.tiles().iter().flat_map(|(_, s)| s).copied().collect();

			let mut scored: Vec<(usize, usize, usize)> = pool
				.iter()
				.enumerate()
				.map(|(i, (_, srcs))| {
					let overlap = srcs.iter().filter(|s| batch_sources.contains(s)).count();
					let new_sources = srcs.len() - overlap;
					(overlap, new_sources, i)
				})
				.collect();

			// Sort descending by overlap, ascending by new_sources. Stable sort keeps
			// pool order as the final deterministic tiebreak.
			scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

			// Step 6: if no picks fit, finish the batch.
			if scored.is_empty() {
				break;
			}

			// Step 5 (cont.): take up to `room` best candidates.
			let mut picks: Vec<usize> = scored.into_iter().take(room).map(|(_, _, i)| i).collect();

			// Swap-remove in descending index order to keep earlier indices valid.
			picks.sort_unstable_by(|a, b| b.cmp(a));
			for i in picks {
				let (c, s) = pool.swap_remove(i);
				batch.push(c, s);
			}
			// Step 7: loop — recompute peak under the new membership.
		}

		batches.push(batch);
	}

	let total_source_opens: usize = batches.iter().map(|b| b.sources().len()).sum();
	log::debug!(
		"partitioned {} tiles into {} batches ({} total source-opens)",
		total_tiles,
		batches.len(),
		total_source_opens
	);

	batches
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
	let total_source_reads: u64 = batches.iter().map(|batch| batch.sources().len() as u64).sum();

	let progress = runtime.create_progress("pass 2/2: compositing tiles", total_source_reads);
	let concurrency = ConcurrencyLimits::default().cpu_bound;

	for batch in batches {
		composite_one_batch(batch, paths, config, sink, runtime, concurrency, &progress).await?;
	}

	progress.finish();
	Ok(())
}

/// Composite a single batch: fetch tiles from each needed source, composite, and
/// flush each tile as soon as its last contributing source has been processed.
///
/// Flushing mid-batch keeps peak memory bounded by `TileBatch::max_tiles_in_memory`
/// — otherwise every tile would sit in the buffer until the very last source.
async fn composite_one_batch(
	batch: &TileBatch,
	paths: &[String],
	config: &AssembleConfig,
	sink: &Arc<Box<dyn TileSink>>,
	runtime: &TilesRuntime,
	concurrency: usize,
	progress: &versatiles_container::ProgressHandle,
) -> Result<()> {
	let tiles = batch.tiles();
	let sources_needed = batch.sources();

	// For each source, the Hilbert keys of tiles whose *last* contributing source is
	// that source — those tiles are safe to flush right after the source step ends.
	let mut flush_keys: HashMap<usize, Vec<u64>> = HashMap::new();
	for (coord, srcs) in tiles {
		let last = *srcs.last().expect("tile has at least one source");
		flush_keys.entry(last).or_default().push(coord.get_hilbert_index()?);
	}

	let buffer = Arc::new(TranslucentBuffer::new());
	let mut prefetched: Option<Vec<(TileCoord, Tile)>> = None;

	let batch_arc = Arc::new(tiles.to_vec());
	let paths_arc = Arc::new(paths.to_vec());

	for (pos, &source_idx) in sources_needed.iter().enumerate() {
		// Use pre-fetched tiles if available, otherwise fetch now
		let fetched_tiles = if let Some(tiles) = prefetched.take() {
			tiles
		} else {
			fetch_source_tiles(source_idx, tiles, paths, runtime).await?
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

		// Flush tiles whose last contributing source is this one.
		if let Some(keys) = flush_keys.remove(&source_idx) {
			let flushed = buffer.remove_many(&keys);
			if !flushed.is_empty() {
				let prepared = encode_tiles_parallel(flushed, config);
				for result in prepared {
					let (coord, blob) = result?;
					sink.write_tile(&coord, &blob)?;
				}
			}
		}

		progress.inc(1);
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use std::sync::atomic::{AtomicUsize, Ordering};
	use tempfile::TempDir;
	use versatiles_container::{MockReader, MockReaderProfile, TileSource, TilesRuntime};
	use versatiles_core::TileFormat;
	use versatiles_core::{Blob, TileCompression, TileJSON};
	use versatiles_image::{DynamicImage, ImageBuffer};

	fn tc(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord { level, x, y }
	}

	fn opaque_rgb_tile() -> Tile {
		let data = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128];
		let img = DynamicImage::ImageRgb8(ImageBuffer::from_vec(2, 2, data).unwrap());
		Tile::from_image(img, TileFormat::PNG).unwrap()
	}

	fn translucent_rgba_tile(alpha: u8) -> Tile {
		let data = vec![
			255, 0, 0, alpha, 0, 255, 0, alpha, 0, 0, 255, alpha, 128, 128, 128, alpha,
		];
		let img = DynamicImage::ImageRgba8(ImageBuffer::from_vec(2, 2, data).unwrap());
		Tile::from_image(img, TileFormat::PNG).unwrap()
	}

	fn assemble_config() -> AssembleConfig {
		AssembleConfig {
			quality: [Some(75); 32],
			lossless: false,
			tile_format: TileFormat::PNG,
			tile_compression: versatiles_core::TileCompression::Uncompressed,
		}
	}

	// Build sink that we can observe from outside via a shared counter.
	#[allow(clippy::type_complexity)]
	fn make_observable_sink() -> (Arc<Box<dyn TileSink>>, Arc<AtomicUsize>, Arc<Mutex<Vec<TileCoord>>>) {
		let count = Arc::new(AtomicUsize::new(0));
		let coords: Arc<Mutex<Vec<TileCoord>>> = Arc::new(Mutex::new(Vec::new()));
		let count2 = Arc::clone(&count);
		let coords2 = Arc::clone(&coords);

		struct ObsSink {
			count: Arc<AtomicUsize>,
			coords: Arc<Mutex<Vec<TileCoord>>>,
		}
		impl TileSink for ObsSink {
			fn write_tile(&self, coord: &TileCoord, _blob: &Blob) -> Result<()> {
				self.count.fetch_add(1, Ordering::SeqCst);
				self.coords.lock().unwrap().push(*coord);
				Ok(())
			}
			fn finish(self: Box<Self>, _: &TileJSON, _: &TilesRuntime) -> Result<()> {
				Ok(())
			}
		}

		let sink = Arc::new(Box::new(ObsSink {
			count: count2,
			coords: coords2,
		}) as Box<dyn TileSink>);
		(sink, count, coords)
	}

	// ─── classify_and_stream_tiles ───

	#[tokio::test]
	async fn classify_opaque_tiles_are_written_to_sink() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), opaque_rgb_tile()), (tc(1, 0, 0), opaque_rgb_tile())]);
		let (sink, count, coords) = make_observable_sink();
		let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();

		let translucent = classify_and_stream_tiles(stream, assemble_config(), sink, done.clone())
			.await
			.unwrap();

		assert_eq!(count.load(Ordering::SeqCst), 2, "two opaque tiles should be written");
		assert!(translucent.is_empty(), "no translucent tiles expected");
		let written = coords.lock().unwrap().clone();
		assert!(written.contains(&tc(0, 0, 0)));
		assert!(written.contains(&tc(1, 0, 0)));
		// coords should be in done set
		assert!(done.lock().unwrap().contains(&tc(0, 0, 0)));
		assert!(done.lock().unwrap().contains(&tc(1, 0, 0)));
	}

	#[tokio::test]
	async fn classify_translucent_tiles_are_returned_not_written() {
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), translucent_rgba_tile(128))]);
		let (sink, count, _) = make_observable_sink();
		let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();

		let translucent = classify_and_stream_tiles(stream, assemble_config(), sink, done)
			.await
			.unwrap();

		assert_eq!(count.load(Ordering::SeqCst), 0, "translucent tile must not be written");
		assert_eq!(translucent, vec![tc(0, 0, 0)]);
	}

	#[tokio::test]
	async fn classify_empty_tiles_are_skipped() {
		// alpha=0 means fully transparent → is_empty() returns true
		let stream = TileStream::from_vec(vec![(tc(0, 0, 0), translucent_rgba_tile(0))]);
		let (sink, count, _) = make_observable_sink();
		let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();

		let translucent = classify_and_stream_tiles(stream, assemble_config(), sink, done)
			.await
			.unwrap();

		assert_eq!(count.load(Ordering::SeqCst), 0);
		assert!(translucent.is_empty(), "empty tile should be skipped entirely");
	}

	#[tokio::test]
	async fn classify_already_done_coords_are_skipped() {
		let coord = tc(0, 0, 0);
		let stream = TileStream::from_vec(vec![(coord, opaque_rgb_tile())]);
		let (sink, count, _) = make_observable_sink();
		let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();
		done.lock().unwrap().insert(coord);

		let translucent = classify_and_stream_tiles(stream, assemble_config(), sink, done)
			.await
			.unwrap();

		assert_eq!(count.load(Ordering::SeqCst), 0, "already-done tile should be skipped");
		assert!(translucent.is_empty());
	}

	#[tokio::test]
	async fn classify_mixed_stream() {
		let stream = TileStream::from_vec(vec![
			(tc(0, 0, 0), opaque_rgb_tile()),
			(tc(1, 0, 0), translucent_rgba_tile(128)),
			(tc(2, 0, 0), translucent_rgba_tile(0)), // empty
			(tc(3, 0, 0), opaque_rgb_tile()),
		]);
		let (sink, count, _) = make_observable_sink();
		let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();

		let translucent = classify_and_stream_tiles(stream, assemble_config(), sink, done)
			.await
			.unwrap();

		assert_eq!(count.load(Ordering::SeqCst), 2, "two opaque tiles written");
		assert_eq!(translucent.len(), 1);
		assert!(translucent.contains(&tc(1, 0, 0)));
	}

	#[tokio::test]
	async fn classify_empty_stream_returns_empty() {
		let stream = TileStream::from_vec(vec![]);
		let (sink, count, _) = make_observable_sink();
		let done: Arc<Mutex<HashSet<TileCoord>>> = Arc::default();

		let translucent = classify_and_stream_tiles(stream, assemble_config(), sink, done)
			.await
			.unwrap();

		assert_eq!(count.load(Ordering::SeqCst), 0);
		assert!(translucent.is_empty());
	}

	// ─── scan_sources / composite_batches / composite_one_batch ───

	/// Write a small MockReader::Png versatiles file to a temp path and return (TempDir, path).
	async fn create_png_versatiles(runtime: &TilesRuntime) -> (TempDir, String) {
		let reader = MockReader::new_mock_profile(MockReaderProfile::Png).unwrap().boxed();
		let dir = TempDir::new().unwrap();
		let path = dir.path().join("src.versatiles").to_string_lossy().into_owned();
		runtime.write_to_str(Arc::new(reader), &path).await.unwrap();
		(dir, path)
	}

	fn png_quality() -> [Option<u8>; 32] {
		[Some(75); 32]
	}

	#[tokio::test]
	async fn scan_sources_no_sources_returns_error() {
		let runtime = TilesRuntime::default();
		let dir = TempDir::new().unwrap();
		let output = dir.path().join("out.versatiles").to_string_lossy().into_owned();
		let err = scan_sources(&output, &[], &png_quality(), false, None, None, &runtime)
			.await
			.err()
			.expect("expected an error");
		assert!(err.to_string().contains("no sources found"), "got: {err}");
	}

	#[tokio::test]
	async fn scan_sources_all_opaque_writes_to_done() {
		let runtime = TilesRuntime::default();
		let (_src_dir, src_path) = create_png_versatiles(&runtime).await;
		let out_dir = TempDir::new().unwrap();
		let output = out_dir.path().join("out.versatiles").to_string_lossy().into_owned();

		let result = scan_sources(&output, &[src_path], &png_quality(), false, None, None, &runtime)
			.await
			.unwrap();

		assert!(!result.done.is_empty(), "opaque tiles should all land in done");
		assert!(result.translucent_map.is_empty(), "no translucent tiles expected");
		assert_eq!(result.config.tile_format, TileFormat::PNG);
		assert_eq!(result.config.tile_compression, TileCompression::Uncompressed);
	}

	#[tokio::test]
	async fn scan_sources_zoom_filter_excludes_all_tiles() {
		let runtime = TilesRuntime::default();
		let (_src_dir, src_path) = create_png_versatiles(&runtime).await;
		let out_dir = TempDir::new().unwrap();
		let output = out_dir.path().join("out.versatiles").to_string_lossy().into_owned();

		// MockReader has tiles at levels 2–6; filter to levels 10–14 → no tiles stream
		let result = scan_sources(
			&output,
			&[src_path],
			&png_quality(),
			false,
			Some(10),
			Some(14),
			&runtime,
		)
		.await
		.unwrap();

		assert!(result.done.is_empty());
		assert!(result.translucent_map.is_empty());
	}

	#[tokio::test]
	async fn composite_batches_empty_batches_is_ok() {
		let runtime = TilesRuntime::default();
		let (_src_dir, src_path) = create_png_versatiles(&runtime).await;
		let (sink, _, _) = make_observable_sink();
		let config = AssembleConfig {
			quality: png_quality(),
			lossless: false,
			tile_format: TileFormat::PNG,
			tile_compression: TileCompression::Uncompressed,
		};

		composite_batches(&[], &[src_path], &config, &sink, &runtime)
			.await
			.unwrap();
	}

	#[tokio::test]
	async fn composite_one_batch_composites_and_writes() {
		let runtime = TilesRuntime::default();
		// Create two identical PNG sources
		let (_dir0, src0) = create_png_versatiles(&runtime).await;
		let (_dir1, src1) = create_png_versatiles(&runtime).await;
		let paths = vec![src0, src1];

		let (sink, count, _) = make_observable_sink();
		let config = AssembleConfig {
			quality: png_quality(),
			lossless: false,
			tile_format: TileFormat::PNG,
			tile_compression: TileCompression::Uncompressed,
		};

		// Use a tile coord that exists in both sources (level 4 is "full" in MockReader)
		let coord = TileCoord::new(4, 0, 0).unwrap();
		let mut batch = TileBatch::new();
		batch.push(coord, vec![0, 1]);

		let progress = runtime.create_progress("test", 2);
		composite_one_batch(&batch, &paths, &config, &sink, &runtime, 4, &progress)
			.await
			.unwrap();

		assert_eq!(
			count.load(Ordering::SeqCst),
			1,
			"composited tile should be written to sink"
		);
	}

	#[tokio::test]
	async fn composite_one_batch_flushes_incrementally() {
		// With a batch whose tiles' last-source is spread across multiple sources,
		// the sink must receive writes *during* the source loop — not only at the end.
		let runtime = TilesRuntime::default();
		let (_d0, src0) = create_png_versatiles(&runtime).await;
		let (_d1, src1) = create_png_versatiles(&runtime).await;
		let (_d2, src2) = create_png_versatiles(&runtime).await;
		let paths = vec![src0, src1, src2];

		let (sink, count, _coords) = make_observable_sink();
		let config = AssembleConfig {
			quality: png_quality(),
			lossless: false,
			tile_format: TileFormat::PNG,
			tile_compression: TileCompression::Uncompressed,
		};

		// Two tiles with different last-sources: tile_a ends at source 1, tile_b at source 2.
		let mut batch = TileBatch::new();
		batch.push(TileCoord::new(4, 0, 0).unwrap(), vec![0, 1]);
		batch.push(TileCoord::new(4, 1, 0).unwrap(), vec![0, 2]);

		let progress = runtime.create_progress("test", 3);
		composite_one_batch(&batch, &paths, &config, &sink, &runtime, 4, &progress)
			.await
			.unwrap();

		assert_eq!(count.load(Ordering::SeqCst), 2, "both tiles should be flushed");
	}

	fn make_translucent_map(entries: &[(TileCoord, &[usize])]) -> HashMap<TileCoord, Vec<usize>> {
		entries.iter().map(|(c, s)| (*c, s.to_vec())).collect()
	}

	fn batch_of(entries: &[(TileCoord, &[usize])]) -> TileBatch {
		let mut batch = TileBatch::new();
		for (c, s) in entries {
			batch.push(*c, s.to_vec());
		}
		batch
	}

	// ─── TileBatch::max_tiles_in_memory ───

	#[test]
	fn max_tiles_in_memory_empty() {
		assert_eq!(TileBatch::new().max_tiles_in_memory(), 0);
	}

	#[test]
	fn max_tiles_in_memory_single_tile() {
		let batch = batch_of(&[(tc(0, 0, 0), &[0, 1, 2])]);
		assert_eq!(batch.max_tiles_in_memory(), 1);
	}

	#[test]
	fn max_tiles_in_memory_disjoint_sources() {
		// Three tiles with disjoint sources processed sequentially: each flushes
		// before the next one is loaded, so the peak is 1, not 3.
		let batch = batch_of(&[(tc(0, 0, 0), &[0]), (tc(0, 1, 0), &[1]), (tc(0, 2, 0), &[2])]);
		assert_eq!(batch.max_tiles_in_memory(), 1);
	}

	#[test]
	fn max_tiles_in_memory_overlapping_span() {
		// Both tiles span sources 0..=2, so both are alive for every step.
		let batch = batch_of(&[(tc(0, 0, 0), &[0, 2]), (tc(0, 1, 0), &[0, 2])]);
		assert_eq!(batch.max_tiles_in_memory(), 2);
	}

	#[test]
	fn max_tiles_in_memory_staggered_intervals() {
		// Timeline sources=[0,1,2,3]; tiles cover [0,1], [1,2], [2,3].
		// Peak is 2 at step 1 (tiles 0 and 1 overlapping) and step 2 (tiles 1 and 2).
		let batch = batch_of(&[(tc(0, 0, 0), &[0, 1]), (tc(0, 1, 0), &[1, 2]), (tc(0, 2, 0), &[2, 3])]);
		assert_eq!(batch.max_tiles_in_memory(), 2);
	}

	#[test]
	fn max_tiles_in_memory_sparse_source_indices() {
		// Actual source indices 0 and 100 are not positions — `max_tiles_in_memory`
		// uses their positions in the sorted source list.
		let batch = batch_of(&[(tc(0, 0, 0), &[0, 100]), (tc(0, 1, 0), &[50])]);
		// Timeline positions: {0, 50, 100} → positions (0, 2). Tile 0 spans 0..=2,
		// tile 1 at position 1. Peak at step 1 = 2.
		assert_eq!(batch.max_tiles_in_memory(), 2);
	}

	// ─── prepare_batches: packing density ───

	#[test]
	fn prepare_batches_packs_disjoint_tiles_densely() {
		// Ten tiles whose sources are all distinct — the old fixed-size packer would
		// produce `ceil(10 / batch_size)` batches. Because each tile flushes as its
		// sole source finishes, the new packer fits them all in a single batch.
		let entries: Vec<(TileCoord, &[usize])> = (0..10).map(|i| (tc(0, i, 0), &[0][..])).collect();
		// Give each tile its own source index.
		let map: HashMap<TileCoord, Vec<usize>> = entries.iter().enumerate().map(|(i, (c, _))| (*c, vec![i])).collect();

		// batch_size=2, but per-tile peak is 1 → all 10 fit in one batch.
		let max_buffer = 256 * 256 * 4 * 2;
		let batches = prepare_batches(map, HashSet::new(), 256, max_buffer, 10);
		assert_eq!(batches.len(), 1, "disjoint tiles should pack into a single batch");
		assert_eq!(batches[0].len(), 10);
		assert_eq!(batches[0].max_tiles_in_memory(), 1);
	}

	#[test]
	fn prepare_batches_respects_peak_under_overlap() {
		// Five tiles all covering source 0 — peak must equal batch size.
		let map = make_translucent_map(&[
			(tc(0, 0, 0), &[0]),
			(tc(0, 1, 0), &[0]),
			(tc(0, 2, 0), &[0]),
			(tc(0, 3, 0), &[0]),
			(tc(0, 4, 0), &[0]),
		]);
		// batch_size=2 → expect peak ≤ 2 in every batch.
		let max_buffer = 256 * 256 * 4 * 2;
		let batches = prepare_batches(map, HashSet::new(), 256, max_buffer, 1);
		let total: usize = batches.iter().map(TileBatch::len).sum();
		assert_eq!(total, 5);
		for batch in &batches {
			assert!(batch.max_tiles_in_memory() <= 2);
		}
	}

	#[test]
	fn prepare_batches_returns_none_when_empty() {
		let map = HashMap::new();
		let done = HashSet::new();
		assert!(prepare_batches(map, done, 256, 1_000_000, 3).is_empty());
	}

	#[test]
	fn prepare_batches_returns_none_when_all_done() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0, 1]), (tc(1, 0, 0), &[1, 2])]);
		let done: HashSet<TileCoord> = [tc(0, 0, 0), tc(1, 0, 0)].into_iter().collect();
		assert!(prepare_batches(map, done, 256, 1_000_000, 3).is_empty());
	}

	#[test]
	fn prepare_batches_prunes_done_coords() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1]), (tc(2, 0, 0), &[2])]);
		let done: HashSet<TileCoord> = [tc(0, 0, 0)].into_iter().collect();
		let batches = prepare_batches(map, done, 256, 0, 3);
		let total_tiles: usize = batches.iter().map(TileBatch::len).sum();
		assert_eq!(total_tiles, 2);
	}

	#[test]
	fn prepare_batches_unlimited_buffer_single_batch() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1]), (tc(2, 0, 0), &[2])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 0, 3);
		// max_buffer_size=0 means unlimited → single batch
		assert_eq!(batches.len(), 1);
		// Each tile has disjoint sources so the peak is 1: tiles flush as their sole source
		// finishes, even though the batch holds all 3.
		assert_eq!(batches[0].len(), 3);
	}

	#[test]
	fn prepare_batches_respects_buffer_size() {
		// tile_dim=256, so each tile = 256*256*4 = 262144 bytes
		// max_buffer_size=300000 → batch_size=1
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1]), (tc(1, 1, 0), &[2])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 300_000, 3);
		assert_eq!(batches.len(), 3);
		for batch in &batches {
			assert_eq!(batch.len(), 1);
		}
	}

	#[test]
	fn prepare_batches_preserves_source_indices() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0, 2]), (tc(1, 0, 0), &[1, 3])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 0, 4);
		let all_tiles: Vec<_> = batches.into_iter().flat_map(TileBatch::into_tiles).collect();
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
		// tile_dim=256, max_buffer_size=256*256*4*5 means peak memory ≤ 5 tiles.
		let max_buffer = 256 * 256 * 4 * 5;
		let batches = prepare_batches(map, HashSet::new(), 256, max_buffer, 3);

		let total_tiles: usize = batches.iter().map(TileBatch::len).sum();
		assert_eq!(total_tiles, 20);

		for batch in &batches {
			let peak = batch.max_tiles_in_memory();
			assert!(peak <= 5, "batch peak is {peak}, expected <= 5");
		}
	}

	#[test]
	fn prepare_batches_single_tile() {
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 0, 1);
		assert_eq!(batches.len(), 1);
		assert_eq!(batches[0].len(), 1);
		assert_eq!(batches[0].tiles()[0].0, tc(0, 0, 0));
		assert_eq!(batches[0].tiles()[0].1, vec![0]);
	}

	#[test]
	fn prepare_batches_large_tile_dim() {
		// tile_dim=512, each tile = 512*512*4 = 1048576 bytes
		// max_buffer_size=2000000 → batch_size=1
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1])]);
		let batches = prepare_batches(map, HashSet::new(), 512, 2_000_000, 2);
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
		let batches = prepare_batches(map, HashSet::new(), 64, 50_000, 4);
		let total: usize = batches.iter().map(TileBatch::len).sum();
		assert_eq!(total, 7);
		for batch in &batches {
			assert!(batch.max_tiles_in_memory() <= 3);
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
		let batches = prepare_batches(map, done, 256, 0, 2);
		let total: usize = batches.iter().map(TileBatch::len).sum();
		assert_eq!(total, 2);
	}

	#[test]
	fn prepare_batches_done_not_in_map_ignored() {
		// Done set has coords not in map — should not cause issues
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0])]);
		let done: HashSet<TileCoord> = [tc(5, 5, 5), tc(10, 10, 10)].into_iter().collect();
		let batches = prepare_batches(map, done, 256, 0, 1);
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
		let batches = prepare_batches(map, HashSet::new(), 256, max_buffer, 10);

		let total: usize = batches.iter().map(TileBatch::len).sum();
		assert_eq!(total, 100);

		for batch in &batches {
			assert!(batch.max_tiles_in_memory() <= 25);
		}
	}

	#[test]
	fn prepare_batches_batch_size_equals_total() {
		// Exact boundary: batch_size == total_tiles
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(0, 1, 0), &[1]), (tc(0, 2, 0), &[2])]);
		// tile_dim=256, max_buffer_size = exactly 3 tiles
		let max_buffer = 256 * 256 * 4 * 3;
		let batches = prepare_batches(map, HashSet::new(), 256, max_buffer, 3);
		assert_eq!(batches.len(), 1);
		assert_eq!(batches[0].len(), 3);
	}

	#[test]
	fn prepare_batches_sources_sorted_in_output() {
		// Source indices must be ascending for max_tiles_in_memory to work; `prepare_batches` sorts them.
		let map = make_translucent_map(&[(tc(0, 0, 0), &[3, 1, 2])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 0, 5);
		assert_eq!(batches[0].tiles()[0].1, vec![1, 2, 3]);
	}

	#[test]
	fn prepare_batches_max_buffer_one_byte() {
		// Absurdly small buffer → batch_size=1 (clamped to at least 1)
		let map = make_translucent_map(&[(tc(0, 0, 0), &[0]), (tc(1, 0, 0), &[1])]);
		let batches = prepare_batches(map, HashSet::new(), 256, 1, 2);
		// Each tile gets its own batch
		assert_eq!(batches.len(), 2);
	}
}
