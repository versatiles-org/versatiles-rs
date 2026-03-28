//! Tile processing helpers: validation, compositing, encoding, and fetching.
//!
//! Encoding requirements:
//!
//! - **Opaque tiles** must never be re-encoded. Their original blob is written
//!   to the sink byte-for-byte (only recompressed if the container compression
//!   differs, which `into_blob` handles as a no-op when it already matches).
//!
//! - **Translucent tiles** are re-encoded exactly once as lossy WebP (or
//!   lossless when `--lossless` is set). The single encoding happens during
//!   the flush step in `encode_tiles_parallel`, which calls `change_format`
//!   to set format + quality, followed by `into_blob` → `materialize_blob`
//!   to produce the blob. Compositing in `composite_two_tiles` deliberately
//!   does NOT encode — it keeps the merged image as raw content so that the
//!   flush step is the only place where lossy compression is applied.

use super::AssembleConfig;
use anyhow::{Context, Result, ensure};
use futures::StreamExt;
use versatiles_container::{Tile, TilesRuntime};
use versatiles_core::{Blob, ConcurrencyLimits, TileCoord, TileFormat};
use versatiles_image::traits::DynamicImageTraitOperation;

pub(super) fn validate_source_format(
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
pub(super) fn composite_two_tiles(base: Tile, top: Tile) -> Result<Tile> {
	let base_image = base.into_image()?;
	let top_image = top.into_image()?;

	let mut result = base_image;
	result.overlay_additive(&top_image)?;

	// Keep as raw image — `encode_tiles_parallel` will set format + quality later.
	Tile::from_image(result, TileFormat::WEBP)
}

/// Write an opaque tile's original blob to the sink without re-encoding.
pub(super) fn write_opaque_blob(tile: Tile, config: &AssembleConfig) -> Result<Blob> {
	tile.into_blob(config.tile_compression)
}

/// Re-encode translucent tiles as WebP in parallel and compress for the output container.
///
/// This is the single place where lossy (or lossless) WebP compression is applied.
/// Tiles coming from `composite_two_tiles` carry raw image content (no blob),
/// so `change_format` + `into_blob` produces the one-and-only encoded blob.
/// Single-source tiles still hold their original source blob, which is decoded
/// and re-encoded here as well.
pub(super) fn encode_tiles_parallel(
	tiles: Vec<(TileCoord, Tile)>,
	config: &AssembleConfig,
) -> Vec<Result<(TileCoord, Blob)>> {
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

/// Read all tiles for a given source that are relevant to the batch.
///
/// Returns `(coord, tile)` pairs with empty tiles already filtered out.
/// Used both for direct fetching and for pre-fetching the next source.
pub(super) async fn fetch_source_tiles(
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
