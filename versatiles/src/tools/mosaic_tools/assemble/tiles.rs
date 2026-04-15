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
				match reader.tile(&coord).await? {
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

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::TileCompression;
	use versatiles_image::{DynamicImage, ImageBuffer};

	fn test_config() -> AssembleConfig {
		AssembleConfig {
			quality: [Some(75); 32],
			lossless: false,
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
		}
	}

	fn opaque_rgb_tile() -> Tile {
		// 2x2 RGB image (no alpha → opaque)
		let data = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128];
		let img = DynamicImage::ImageRgb8(ImageBuffer::from_vec(2, 2, data).unwrap());
		Tile::from_image(img, TileFormat::PNG).unwrap()
	}

	fn translucent_rgba_tile(alpha: u8) -> Tile {
		// 2x2 RGBA image with given alpha
		let data = vec![
			255, 0, 0, alpha, 0, 255, 0, alpha, 0, 0, 255, alpha, 128, 128, 128, alpha,
		];
		let img = DynamicImage::ImageRgba8(ImageBuffer::from_vec(2, 2, data).unwrap());
		Tile::from_image(img, TileFormat::PNG).unwrap()
	}

	fn coord(level: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(level, x, y).unwrap()
	}

	#[test]
	fn validate_source_format_matching() {
		let config = test_config();
		let metadata = versatiles_container::TileSourceMetadata {
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
			..Default::default()
		};
		assert!(validate_source_format("test.versatiles", &metadata, &config).is_ok());
	}

	#[test]
	fn validate_source_format_mismatched_format() {
		let config = test_config();
		let metadata = versatiles_container::TileSourceMetadata {
			tile_format: TileFormat::PNG,
			tile_compression: TileCompression::Uncompressed,
			..Default::default()
		};
		let err = validate_source_format("bad.versatiles", &metadata, &config).unwrap_err();
		assert!(err.to_string().contains("tile format"));
	}

	#[test]
	fn validate_source_format_mismatched_compression() {
		let config = test_config();
		let metadata = versatiles_container::TileSourceMetadata {
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Gzip,
			..Default::default()
		};
		let err = validate_source_format("bad.versatiles", &metadata, &config).unwrap_err();
		assert!(err.to_string().contains("tile compression"));
	}

	#[test]
	fn write_opaque_blob_produces_data() -> Result<()> {
		let config = test_config();
		let tile = opaque_rgb_tile();
		let blob = write_opaque_blob(tile, &config)?;
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn composite_two_tiles_produces_result() -> Result<()> {
		let base = translucent_rgba_tile(128);
		let top = translucent_rgba_tile(128);
		let result = composite_two_tiles(base, top)?;
		// Result should be a tile with image content
		let img = result.into_image()?;
		assert_eq!(img.width(), 2);
		assert_eq!(img.height(), 2);
		Ok(())
	}

	#[test]
	fn composite_preserves_dimensions() -> Result<()> {
		let base = translucent_rgba_tile(64);
		let top = translucent_rgba_tile(200);
		let result = composite_two_tiles(base, top)?;
		let img = result.into_image()?;
		assert_eq!(img.width(), 2);
		assert_eq!(img.height(), 2);
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_produces_blobs() -> Result<()> {
		let config = test_config();
		let tiles = vec![
			(coord(0, 0, 0), opaque_rgb_tile()),
			(coord(1, 0, 0), opaque_rgb_tile()),
			(coord(1, 1, 0), opaque_rgb_tile()),
		];

		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 3);
		for result in results {
			let (_, blob) = result?;
			assert!(!blob.is_empty());
		}
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_lossless() -> Result<()> {
		let config = AssembleConfig {
			lossless: true,
			..test_config()
		};
		let tiles = vec![(coord(0, 0, 0), opaque_rgb_tile())];

		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 1);
		let (c, blob) = results.into_iter().next().unwrap()?;
		assert_eq!(c, coord(0, 0, 0));
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_empty_input() {
		let config = test_config();
		let results = encode_tiles_parallel(vec![], &config);
		assert!(results.is_empty());
	}

	#[test]
	fn encode_tiles_parallel_uses_zoom_quality() -> Result<()> {
		let mut quality = [None; 32];
		quality[5] = Some(50);
		let config = AssembleConfig {
			quality,
			lossless: false,
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
		};

		let tiles = vec![(coord(5, 0, 0), opaque_rgb_tile())];
		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 1);
		let (c, blob) = results.into_iter().next().unwrap()?;
		assert_eq!(c.level, 5);
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn write_opaque_blob_with_gzip_compression() -> Result<()> {
		let config = AssembleConfig {
			tile_compression: TileCompression::Gzip,
			..test_config()
		};
		let tile = opaque_rgb_tile();
		let blob = write_opaque_blob(tile, &config)?;
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn composite_opaque_over_translucent() -> Result<()> {
		let base = translucent_rgba_tile(128);
		let top = opaque_rgb_tile();
		let result = composite_two_tiles(base, top)?;
		let img = result.into_image()?;
		assert_eq!(img.width(), 2);
		assert_eq!(img.height(), 2);
		Ok(())
	}

	#[test]
	fn composite_translucent_over_opaque() -> Result<()> {
		let base = opaque_rgb_tile();
		let top = translucent_rgba_tile(128);
		let result = composite_two_tiles(base, top)?;
		let img = result.into_image()?;
		assert_eq!(img.width(), 2);
		assert_eq!(img.height(), 2);
		Ok(())
	}

	#[test]
	fn composite_fully_transparent_over_opaque() -> Result<()> {
		let base = opaque_rgb_tile();
		let top = translucent_rgba_tile(0); // fully transparent
		let result = composite_two_tiles(base, top)?;
		let img = result.into_image()?;
		assert_eq!((img.width(), img.height()), (2, 2));
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_preserves_coords() -> Result<()> {
		let config = test_config();
		let coords = [coord(3, 5, 7), coord(8, 100, 200), coord(0, 0, 0)];
		let tiles: Vec<_> = coords.iter().map(|c| (*c, opaque_rgb_tile())).collect();

		let results = encode_tiles_parallel(tiles, &config);
		let result_coords: Vec<TileCoord> = results.into_iter().map(|r| r.unwrap().0).collect();
		assert_eq!(result_coords, coords);
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_with_gzip() -> Result<()> {
		let config = AssembleConfig {
			tile_compression: TileCompression::Gzip,
			..test_config()
		};
		let tiles = vec![(coord(0, 0, 0), opaque_rgb_tile())];
		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 1);
		let (_, blob) = results.into_iter().next().unwrap()?;
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn validate_source_format_both_mismatched() {
		let config = test_config();
		let metadata = versatiles_container::TileSourceMetadata {
			tile_format: TileFormat::PNG,
			tile_compression: TileCompression::Gzip,
			..Default::default()
		};
		// First check fires (format), so error mentions format
		let err = validate_source_format("bad.versatiles", &metadata, &config).unwrap_err();
		assert!(err.to_string().contains("tile format"));
	}

	#[test]
	fn validate_source_format_includes_path_in_error() {
		let config = test_config();
		let metadata = versatiles_container::TileSourceMetadata {
			tile_format: TileFormat::PNG,
			..Default::default()
		};
		let err = validate_source_format("my/special/path.versatiles", &metadata, &config).unwrap_err();
		assert!(err.to_string().contains("my/special/path.versatiles"));
	}

	#[test]
	fn encode_tiles_parallel_many_tiles_chunked() -> Result<()> {
		// More tiles than typical cpu_bound limit to exercise the chunking loop
		let config = test_config();
		let tiles: Vec<_> = (0..50u32).map(|i| (coord(8, i, 0), opaque_rgb_tile())).collect();

		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 50);
		for r in &results {
			assert!(r.is_ok());
		}
		Ok(())
	}

	#[test]
	fn composite_two_opaque_tiles() -> Result<()> {
		let base = opaque_rgb_tile();
		let top = opaque_rgb_tile();
		let result = composite_two_tiles(base, top)?;
		let img = result.into_image()?;
		assert_eq!((img.width(), img.height()), (2, 2));
		Ok(())
	}

	#[test]
	fn composite_result_produces_blob() -> Result<()> {
		let base = translucent_rgba_tile(100);
		let top = translucent_rgba_tile(100);
		let result = composite_two_tiles(base, top)?;
		// The result should be producible as a blob
		let blob = result.into_blob(TileCompression::Uncompressed)?;
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_single_tile() -> Result<()> {
		let config = test_config();
		let tiles = vec![(coord(0, 0, 0), opaque_rgb_tile())];
		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 1);
		let (c, blob) = results.into_iter().next().unwrap()?;
		assert_eq!(c, coord(0, 0, 0));
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn write_opaque_blob_with_brotli() -> Result<()> {
		let config = AssembleConfig {
			tile_compression: TileCompression::Brotli,
			..test_config()
		};
		let tile = opaque_rgb_tile();
		let blob = write_opaque_blob(tile, &config)?;
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn composite_verifies_pixel_blending() -> Result<()> {
		// Base: red at alpha=128, Top: blue at alpha=127
		// Expected additive blend: alpha_out = 128+127 = 255 (snapped from 255≥250)
		// r = (255*128 + 0*127 + 127) / 255 = 128
		// b = (0*128 + 255*127 + 127) / 255 = 127
		let base_data = vec![255, 0, 0, 128, 255, 0, 0, 128, 255, 0, 0, 128, 255, 0, 0, 128];
		let top_data = vec![0, 0, 255, 127, 0, 0, 255, 127, 0, 0, 255, 127, 0, 0, 255, 127];
		let base_img = DynamicImage::ImageRgba8(ImageBuffer::from_vec(2, 2, base_data).unwrap());
		let top_img = DynamicImage::ImageRgba8(ImageBuffer::from_vec(2, 2, top_data).unwrap());
		let base = Tile::from_image(base_img, TileFormat::PNG)?;
		let top = Tile::from_image(top_img, TileFormat::PNG)?;

		let result = composite_two_tiles(base, top)?;
		let img = result.into_image()?;
		use versatiles_image::traits::DynamicImageTraitConvert;
		let px = img.raw_pixel(0, 0);
		assert_eq!(px[3], 255); // alpha snapped to 255
		assert_eq!(px[0], 128); // red component
		assert_eq!(px[2], 127); // blue component
		Ok(())
	}

	#[test]
	fn composite_result_format_is_webp() -> Result<()> {
		let base = translucent_rgba_tile(128);
		let top = translucent_rgba_tile(64);
		let result = composite_two_tiles(base, top)?;
		assert_eq!(result.format(), TileFormat::WEBP);
		Ok(())
	}

	#[test]
	fn composite_transparent_over_transparent() -> Result<()> {
		let base = translucent_rgba_tile(0);
		let top = translucent_rgba_tile(0);
		let result = composite_two_tiles(base, top)?;
		let img = result.into_image()?;
		use versatiles_image::traits::DynamicImageTraitConvert;
		// Both fully transparent → result should be transparent
		let px = img.raw_pixel(0, 0);
		assert_eq!(px[3], 0);
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_with_composited_tiles() -> Result<()> {
		// Encode tiles that went through composite (the real pipeline path)
		let base = translucent_rgba_tile(128);
		let top = translucent_rgba_tile(64);
		let composited = composite_two_tiles(base, top)?;

		let config = test_config();
		let tiles = vec![(coord(3, 1, 2), composited)];
		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 1);
		let (c, blob) = results.into_iter().next().unwrap()?;
		assert_eq!(c, coord(3, 1, 2));
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_none_quality() -> Result<()> {
		// Quality set to None for a zoom level — should still encode
		let mut quality = [None; 32];
		// level 3 has None quality explicitly
		quality[3] = None;
		let config = AssembleConfig {
			quality,
			lossless: false,
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
		};

		let tiles = vec![(coord(3, 0, 0), opaque_rgb_tile())];
		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 1);
		let (c, blob) = results.into_iter().next().unwrap()?;
		assert_eq!(c.level, 3);
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_with_brotli() -> Result<()> {
		let config = AssembleConfig {
			tile_compression: TileCompression::Brotli,
			..test_config()
		};
		let tiles = vec![(coord(0, 0, 0), opaque_rgb_tile())];
		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 1);
		let (_, blob) = results.into_iter().next().unwrap()?;
		assert!(!blob.is_empty());
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_mixed_zoom_levels() -> Result<()> {
		// Different zoom levels use different quality entries
		let mut quality = [None; 32];
		quality[0] = Some(50);
		quality[5] = Some(90);
		quality[10] = Some(30);
		let config = AssembleConfig {
			quality,
			lossless: false,
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
		};

		let tiles = vec![
			(coord(0, 0, 0), opaque_rgb_tile()),
			(coord(5, 3, 4), opaque_rgb_tile()),
			(coord(10, 100, 200), opaque_rgb_tile()),
		];
		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 3);
		for r in results {
			let (_, blob) = r?;
			assert!(!blob.is_empty());
		}
		Ok(())
	}

	#[test]
	fn encode_tiles_parallel_translucent_tiles() -> Result<()> {
		let config = test_config();
		let tiles = vec![
			(coord(0, 0, 0), translucent_rgba_tile(128)),
			(coord(1, 0, 0), translucent_rgba_tile(64)),
			(coord(2, 0, 0), translucent_rgba_tile(255)),
		];
		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 3);
		for r in results {
			let (_, blob) = r?;
			assert!(!blob.is_empty());
		}
		Ok(())
	}

	#[test]
	fn write_opaque_blob_uncompressed_roundtrip() -> Result<()> {
		let config = test_config();
		let tile = opaque_rgb_tile();
		let original_format = tile.format();
		let blob = write_opaque_blob(tile, &config)?;
		// The blob preserves the original format (not re-encoded)
		let restored = Tile::from_blob(blob, config.tile_compression, original_format);
		let img = restored.into_image()?;
		assert_eq!((img.width(), img.height()), (2, 2));
		Ok(())
	}

	#[test]
	fn validate_all_format_combinations() {
		// Test several format combinations to verify the validation logic
		for fmt in [TileFormat::PNG, TileFormat::WEBP, TileFormat::JPG] {
			for comp in [
				TileCompression::Uncompressed,
				TileCompression::Gzip,
				TileCompression::Brotli,
			] {
				let config = AssembleConfig {
					tile_format: fmt,
					tile_compression: comp,
					..test_config()
				};
				let matching = versatiles_container::TileSourceMetadata {
					tile_format: fmt,
					tile_compression: comp,
					..Default::default()
				};
				assert!(validate_source_format("test", &matching, &config).is_ok());

				// Mismatched format should fail
				let wrong_fmt = versatiles_container::TileSourceMetadata {
					tile_format: if fmt == TileFormat::PNG {
						TileFormat::WEBP
					} else {
						TileFormat::PNG
					},
					tile_compression: comp,
					..Default::default()
				};
				assert!(validate_source_format("test", &wrong_fmt, &config).is_err());
			}
		}
	}
}
