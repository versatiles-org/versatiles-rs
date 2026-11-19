//! Tile processing helpers: validation, compositing, encoding, and fetching.
//!
//! Encoding requirements:
//!
//! - **Opaque tiles** must never be re-encoded. Their original blob is written
//!   to the sink byte-for-byte (only recompressed if the container compression
//!   differs, which `into_blob` handles as a no-op when it already matches).
//!
//! - **A translucent tile only one input has** is written as it was read too,
//!   unless re-encoding is what makes it lossy (`keeps_its_encoding`).
//!
//! - **Composited translucent tiles** are encoded exactly once, in the format
//!   of the inputs, so that the container holds the format it declares. The single
//!   encoding happens during the flush step in `encode_tiles_parallel`, which
//!   calls `change_format` to set format + quality, followed by `into_blob` →
//!   `materialize_blob` to produce the blob. Compositing in
//!   `composite_two_tiles` deliberately does NOT encode — it keeps the merged
//!   image as raw content so that the flush step is the only place where
//!   lossy compression is applied.

use anyhow::{Context, Result, ensure};
use futures::StreamExt;
use versatiles_container::{Tile, TilesRuntime};
use versatiles_core::{
	Blob, ConcurrencyLimits, TileCompression, TileCoord, TileFormat, TileJSON, TileSchema, compression::compress,
};
use versatiles_image::traits::DynamicImageTraitOperation;

use super::AssembleConfig;

pub(super) fn validate_source_format(
	path: &str,
	metadata: &versatiles_container::TileSourceMetadata,
	config: &AssembleConfig,
) -> Result<()> {
	ensure!(
		*metadata.tile_format() == config.tile_format,
		"Source {path} has tile format {:?}, expected {:?}",
		metadata.tile_format(),
		config.tile_format
	);
	ensure!(
		*metadata.tile_compression() == config.tile_compression,
		"Source {path} has tile compression {:?}, expected {:?}",
		metadata.tile_compression(),
		config.tile_compression
	);
	Ok(())
}

/// Refuse an encoding that the format of the inputs cannot do.
///
/// Composited tiles are encoded in the format of the inputs. WebP and PNG can
/// keep a translucent tile exact; AVIF and JPEG cannot. This is checked before
/// the first tile is written rather than when the first edge tile is.
///
/// `--lossless` asks for that exactness. Elevation tiles need it without
/// being asked, as `convert` gives it to them: a lossy encoder changes the
/// low byte of a height. JPEG has no alpha, so its tiles are only ever copied,
/// and elevation tiles in JPEG lose nothing here that they had not lost.
pub(super) fn validate_encoding(format: TileFormat, lossless: bool, elevation: bool) -> Result<()> {
	ensure!(
		!lossless || format.can_be_lossless(),
		"--lossless needs inputs in a format that can be encoded without loss, WebP or PNG, \
		 but they hold {format} tiles. Leave out --lossless, or tile the scenes as WebP or PNG"
	);
	ensure!(
		!elevation || format.can_be_lossless() || format == TileFormat::JPG,
		"the inputs hold elevation tiles in {format}, which has no lossless encoding, so the \
		 tiles composited where two inputs meet would change their heights. \
		 Tile the elevation data as WebP or PNG"
	);
	Ok(())
}

/// Whether a source's tiles hold heights, as its `tile_schema` says.
pub(super) fn holds_elevation(tilejson: &TileJSON) -> bool {
	tilejson.tile_schema.as_ref().is_some_and(TileSchema::is_elevation)
}

/// Refuse a source that holds heights where the first did not, or the other
/// way round: the two are composited differently, and colours blended as
/// heights are as wrong as heights blended as colours.
pub(super) fn validate_source_elevation(path: &str, elevation: bool, config: &AssembleConfig) -> Result<()> {
	let what = |elevation: bool| if elevation { "elevation" } else { "image" };
	ensure!(
		elevation == config.elevation,
		"Source {path} holds {} tiles, but the first source holds {} tiles. \
		 Check the tile_schema of both",
		what(elevation),
		what(config.elevation)
	);
	Ok(())
}

/// Composite two tiles using additive alpha blending (`base` on bottom, `top` on top).
///
/// Elevation tiles are blended as heights, not as colours: see
/// [`overlay_additive_elevation`](DynamicImageTraitOperation::overlay_additive_elevation).
///
/// Returns the merged tile with raw image content (no blob, no encoding),
/// marked as `format`, the format of the inputs. Encoding is deferred to
/// `encode_tiles_parallel` so that lossy compression is applied exactly once.
pub(super) fn composite_two_tiles(base: Tile, top: Tile, format: TileFormat, elevation: bool) -> Result<Tile> {
	let base_image = base.into_image()?;
	let top_image = top.into_image()?;

	let mut result = base_image;
	if elevation {
		result.overlay_additive_elevation(&top_image)?;
	} else {
		result.overlay_additive(&top_image)?;
	}

	// Keep as raw image — `encode_tiles_parallel` will set quality later.
	Tile::from_image(result, format)
}

/// Write an opaque tile's original blob to the sink without re-encoding.
pub(super) fn write_opaque_blob(tile: Tile, config: &AssembleConfig) -> Result<Blob> {
	tile.into_blob(&config.tile_compression)
}

/// Re-encode translucent tiles in parallel and compress for the output container.
///
/// This is the single place where they are encoded, in the format of the
/// inputs, at `--quality` or, with `--lossless`, without loss. PNG ignores the
/// quality, since it is always lossless.
/// Tiles coming from `composite_two_tiles` carry raw image content (no blob),
/// so `change_format` + `into_blob` produces the one-and-only encoded blob.
/// Single-source tiles still hold their original source blob, and keep it
/// unless re-encoding is what makes them lossy: see [`keeps_its_encoding`].
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
						// A tile only one input has: copied, or made lossy to save space.
						let source = if tile.has_blob() {
							if keeps_its_encoding(&mut tile, &coord, cfg)? {
								return Ok((coord, tile.into_blob(&cfg.tile_compression)?));
							}
							Some(tile.as_blob(&TileCompression::Uncompressed)?.clone())
						} else {
							None
						};
						// `--lossless` is for tiles that are still translucent. Along a
						// seam, two half tiles usually composite into an opaque one,
						// which is as lossy as its neighbours (#319).
						// Heights are never encoded lossily, opaque or not.
						let quality = if cfg.elevation || (cfg.lossless && !tile.is_opaque()?) {
							Some(100)
						} else {
							cfg.quality.at(&coord)
						};
						tile.change_format(cfg.tile_format, quality, None)?;
						let encoded = tile.into_blob(&TileCompression::Uncompressed)?;
						// Lossy WebP stores alpha apart from the colours, and on a
						// smooth or mostly empty tile that can take more bytes than
						// the lossless original. Then the original is both better
						// and smaller.
						let blob = match source {
							Some(source) if source.len() <= encoded.len() => source,
							_ => encoded,
						};
						Ok((coord, compress(blob, &cfg.tile_compression)?))
					})
				})
				.collect();
			handles
				.into_iter()
				.map(|h| h.join().expect("worker thread panicked"))
				.collect()
		});
		results.extend(chunk_results);
	}
	results
}

/// Whether a tile only one input has is written as it was read.
///
/// Re-encoding it cannot bring back detail a lossy encoder took, and a
/// lossless encoder gives back the same pixels at a cost: decoding and
/// encoding once more, and for a lossy source a second generation of loss.
/// What re-encoding can do is make a lossless tile lossy, which is what
/// `--quality` does by default to the lossless edge tiles `mosaic tile`
/// writes, to save space; where it would not, `encode_tiles_parallel` keeps
/// the original after all. So a tile keeps its encoding unless it is lossless
/// WebP that is to become lossy: not with `--lossless`, not for heights, and
/// not where `--quality` asks for 100. PNG is lossless either way, and AVIF
/// lossy either way.
fn keeps_its_encoding(tile: &mut Tile, coord: &TileCoord, config: &AssembleConfig) -> Result<bool> {
	if config.tile_format != TileFormat::WEBP
		|| config.elevation
		|| config.lossless
		|| config.quality.at(coord) == Some(100)
	{
		return Ok(true);
	}
	Ok(!is_lossless_webp(tile.as_blob(&TileCompression::Uncompressed)?))
}

/// Whether a WebP blob is encoded without loss: its image is a `VP8L` chunk,
/// not a lossy `VP8 ` one. An extended file (`VP8X`) lists further chunks,
/// such as `ALPH` for the alpha of a lossy image, before the image itself.
fn is_lossless_webp(blob: &Blob) -> bool {
	let bytes = blob.as_slice();
	let mut offset = 12; // after "RIFF", the file size and "WEBP"
	while let Some(header) = bytes.get(offset..offset + 8) {
		let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
		match &header[0..4] {
			b"VP8L" => return true,
			b"VP8 " => return false,
			_ => offset += 8 + size + size % 2, // chunks are padded to an even size
		}
	}
	false
}

/// Read the tiles at `coords` from the container at `path`.
///
/// Returns `(coord, tile)` pairs with empty tiles already filtered out.
/// Used both for direct fetching and for pre-fetching the next source.
pub(super) async fn fetch_source_tiles(
	path: &str,
	coords: Vec<TileCoord>,
	runtime: &TilesRuntime,
) -> Result<Vec<(TileCoord, Tile)>> {
	let reader = runtime
		.reader_from_str(path)
		.await
		.with_context(|| format!("Failed to open container: {path}"))?;

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
	use versatiles_core::TileCompression;
	use versatiles_image::{DynamicImage, ImageBuffer};
	use versatiles_pipeline::{QualityByZoom, TileQuality};

	use super::*;

	fn quality(text: &str) -> TileQuality {
		TileQuality::new(QualityByZoom::try_from(text).unwrap(), None).unwrap()
	}

	fn test_config() -> AssembleConfig {
		AssembleConfig {
			quality: quality("75"),
			lossless: false,
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
			elevation: false,
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
		let metadata = versatiles_container::TileSourceMetadata::new(
			TileFormat::WEBP,
			TileCompression::Uncompressed,
			versatiles_container::Traversal::ANY,
			None,
		);
		assert!(validate_source_format("test.versatiles", &metadata, &config).is_ok());
	}

	#[test]
	fn validate_source_format_mismatched_format() {
		let config = test_config();
		let metadata = versatiles_container::TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			versatiles_container::Traversal::ANY,
			None,
		);
		let err = validate_source_format("bad.versatiles", &metadata, &config).unwrap_err();
		assert!(err.to_string().contains("tile format"));
	}

	#[test]
	fn validate_source_format_mismatched_compression() {
		let config = test_config();
		let metadata = versatiles_container::TileSourceMetadata::new(
			TileFormat::WEBP,
			TileCompression::Gzip,
			versatiles_container::Traversal::ANY,
			None,
		);
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
		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
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
		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
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
		let tiles = vec![(coord(0, 0, 0), translucent_rgba_tile(128))];

		let results = encode_tiles_parallel(tiles, &config);
		assert_eq!(results.len(), 1);
		let (c, blob) = results.into_iter().next().unwrap()?;
		assert_eq!(c, coord(0, 0, 0));
		assert_eq!(webp_chunk(&blob), *b"VP8L", "a translucent tile is lossless");
		Ok(())
	}

	/// The chunk after the RIFF header: `VP8 ` is lossy, `VP8L` lossless, and
	/// `VP8X` the extended format, which carries alpha.
	fn webp_chunk(blob: &Blob) -> [u8; 4] {
		blob.as_slice()[12..16].try_into().unwrap()
	}

	/// A 2×2 tile with one column opaque and the other transparent.
	fn half_tile(left: bool) -> Tile {
		let (a, b) = if left { (255, 0) } else { (0, 255) };
		let data = vec![200, 100, 50, a, 50, 100, 200, b, 120, 60, 30, a, 30, 60, 120, b];
		let img = DynamicImage::ImageRgba8(ImageBuffer::from_vec(2, 2, data).unwrap());
		Tile::from_image(img, TileFormat::PNG).unwrap()
	}

	/// `--lossless` is for translucent tiles, and an opaque one stays lossy (#319).
	#[test]
	fn encode_tiles_parallel_lossless_keeps_an_opaque_tile_lossy() -> Result<()> {
		let config = AssembleConfig {
			lossless: true,
			..test_config()
		};
		let results = encode_tiles_parallel(vec![(coord(0, 0, 0), opaque_rgb_tile())], &config);
		let (_, blob) = results.into_iter().next().unwrap()?;
		assert_eq!(webp_chunk(&blob), *b"VP8 ");
		Ok(())
	}

	/// The seam between two source images: each gives half a tile, and the
	/// two together are opaque, so the tile is as lossy as its neighbours (#319).
	#[test]
	fn a_seam_composited_to_opaque_is_encoded_lossy() -> Result<()> {
		let config = AssembleConfig {
			lossless: true,
			..test_config()
		};
		let seam = composite_two_tiles(half_tile(true), half_tile(false), TileFormat::WEBP, false)?;
		let results = encode_tiles_parallel(vec![(coord(0, 0, 0), seam)], &config);
		let (_, blob) = results.into_iter().next().unwrap()?;
		assert_eq!(webp_chunk(&blob), *b"VP8 ");

		let half = encode_tiles_parallel(vec![(coord(0, 0, 0), half_tile(true))], &config);
		let (_, blob) = half.into_iter().next().unwrap()?;
		assert_eq!(
			webp_chunk(&blob),
			*b"VP8L",
			"a half tile is still translucent, so lossless"
		);
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
		let config = AssembleConfig {
			quality: quality("5:50"),
			lossless: false,
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
			elevation: false,
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
		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
		let img = result.into_image()?;
		assert_eq!(img.width(), 2);
		assert_eq!(img.height(), 2);
		Ok(())
	}

	#[test]
	fn composite_translucent_over_opaque() -> Result<()> {
		let base = opaque_rgb_tile();
		let top = translucent_rgba_tile(128);
		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
		let img = result.into_image()?;
		assert_eq!(img.width(), 2);
		assert_eq!(img.height(), 2);
		Ok(())
	}

	#[test]
	fn composite_fully_transparent_over_opaque() -> Result<()> {
		let base = opaque_rgb_tile();
		let top = translucent_rgba_tile(0); // fully transparent
		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
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
		let metadata = versatiles_container::TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Gzip,
			versatiles_container::Traversal::ANY,
			None,
		);
		// First check fires (format), so error mentions format
		let err = validate_source_format("bad.versatiles", &metadata, &config).unwrap_err();
		assert!(err.to_string().contains("tile format"));
	}

	#[test]
	fn validate_source_format_includes_path_in_error() {
		let config = test_config();
		let metadata = versatiles_container::TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::default(),
			versatiles_container::Traversal::ANY,
			None,
		);
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
		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
		let img = result.into_image()?;
		assert_eq!((img.width(), img.height()), (2, 2));
		Ok(())
	}

	#[test]
	fn composite_result_produces_blob() -> Result<()> {
		let base = translucent_rgba_tile(100);
		let top = translucent_rgba_tile(100);
		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
		// The result should be producible as a blob
		let blob = result.into_blob(&TileCompression::Uncompressed)?;
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

		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
		let img = result.into_image()?;
		use versatiles_image::traits::DynamicImageTraitConvert;
		let px = img.raw_pixel(0, 0);
		assert_eq!(px[3], 255); // alpha snapped to 255
		assert_eq!(px[0], 128); // red component
		assert_eq!(px[2], 127); // blue component
		Ok(())
	}

	#[test]
	fn composite_result_has_the_format_it_is_given() -> Result<()> {
		for format in [TileFormat::WEBP, TileFormat::PNG, TileFormat::AVIF] {
			let result = composite_two_tiles(translucent_rgba_tile(128), translucent_rgba_tile(64), format, false)?;
			assert_eq!(result.format(), format);
		}
		Ok(())
	}

	/// A container declared PNG used to get WebP tiles wherever two inputs
	/// met, since composited tiles were always encoded as WebP.
	#[test]
	fn composited_tiles_are_encoded_in_the_format_of_the_inputs() -> Result<()> {
		/// The format a blob is in, by its first bytes.
		fn sniff(blob: &[u8]) -> Option<TileFormat> {
			if blob.starts_with(b"\x89PNG\r\n\x1a\n") {
				Some(TileFormat::PNG)
			} else if blob.starts_with(b"RIFF") && blob.get(8..12) == Some(b"WEBP") {
				Some(TileFormat::WEBP)
			} else if blob.get(4..12) == Some(b"ftypavif") {
				Some(TileFormat::AVIF)
			} else {
				None
			}
		}
		for format in [TileFormat::PNG, TileFormat::WEBP, TileFormat::AVIF] {
			let config = AssembleConfig {
				tile_format: format,
				..test_config()
			};
			let seam = composite_two_tiles(half_tile(true), half_tile(false), format, false)?;
			let tiles = vec![
				(coord(0, 0, 0), seam),
				(coord(1, 0, 0), half_tile(true)),
				(coord(1, 1, 0), translucent_rgba_tile(128)),
			];
			for result in encode_tiles_parallel(tiles, &config) {
				let (c, blob) = result?;
				assert_eq!(sniff(blob.as_slice()), Some(format), "the tile at {c:?}");
			}
		}
		Ok(())
	}

	#[test]
	fn png_with_lossless_stays_png() -> Result<()> {
		let config = AssembleConfig {
			tile_format: TileFormat::PNG,
			lossless: true,
			..test_config()
		};
		let results = encode_tiles_parallel(vec![(coord(0, 0, 0), translucent_rgba_tile(128))], &config);
		let (_, blob) = results.into_iter().next().unwrap()?;
		assert!(blob.as_slice().starts_with(b"\x89PNG"));
		let image = Tile::from_blob(blob, TileCompression::Uncompressed, TileFormat::PNG).into_image()?;
		assert_eq!(image.as_bytes(), translucent_rgba_tile(128).into_image()?.as_bytes());
		Ok(())
	}

	/// A 1×1 terrain RGB tile with the given raw height and coverage.
	fn elevation_tile(raw: u32, alpha: u8) -> Tile {
		let [_, red, green, blue] = raw.to_be_bytes();
		let data = vec![red, green, blue, alpha];
		let img = DynamicImage::ImageRgba8(ImageBuffer::from_vec(1, 1, data).unwrap());
		Tile::from_image(img, TileFormat::PNG).unwrap()
	}

	/// At a seam between two DEMs, a pixel each covers in part is the mean of
	/// the two heights, not a mix of their bytes.
	#[test]
	fn elevation_tiles_are_composited_as_heights() -> Result<()> {
		let (low, high) = (0x01_00_FF, 0x01_01_00);
		let raw = |tile: Tile| -> Result<u32> {
			let px = tile.into_image()?.to_rgba8().get_pixel(0, 0).0;
			Ok((u32::from(px[0]) << 16) | (u32::from(px[1]) << 8) | u32::from(px[2]))
		};

		let as_heights = composite_two_tiles(
			elevation_tile(low, 127),
			elevation_tile(high, 128),
			TileFormat::PNG,
			true,
		)?;
		assert!((low..=high).contains(&raw(as_heights)?));

		let as_colours = composite_two_tiles(
			elevation_tile(low, 127),
			elevation_tile(high, 128),
			TileFormat::PNG,
			false,
		)?;
		assert!(
			!(low..=high).contains(&raw(as_colours)?),
			"blending the bytes is what goes wrong"
		);
		Ok(())
	}

	#[test]
	fn holds_elevation_follows_the_tile_schema() {
		let with = |schema: Option<TileSchema>| {
			holds_elevation(&TileJSON {
				tile_schema: schema,
				..TileJSON::default()
			})
		};
		assert!(with(Some(TileSchema::RasterDEMMapbox)));
		assert!(with(Some(TileSchema::RasterDEMTerrarium)));
		assert!(!with(Some(TileSchema::RasterRGBA)));
		assert!(!with(None));
	}

	#[test]
	fn sources_must_agree_on_holding_heights() {
		let config = AssembleConfig {
			elevation: true,
			..test_config()
		};
		assert!(validate_source_elevation("dem.versatiles", true, &config).is_ok());
		let message = validate_source_elevation("photo.versatiles", false, &config)
			.unwrap_err()
			.to_string();
		assert!(message.contains("photo.versatiles"), "{message}");
		assert!(
			message.contains("image tiles") && message.contains("elevation tiles"),
			"{message}"
		);
	}

	#[test]
	fn lossless_is_refused_for_formats_that_cannot_keep_it() {
		for format in [TileFormat::WEBP, TileFormat::PNG, TileFormat::AVIF, TileFormat::JPG] {
			assert!(validate_encoding(format, false, false).is_ok());
		}
		assert!(validate_encoding(TileFormat::WEBP, true, false).is_ok());
		assert!(validate_encoding(TileFormat::PNG, true, false).is_ok());
		for format in [TileFormat::AVIF, TileFormat::JPG] {
			let message = validate_encoding(format, true, false).unwrap_err().to_string();
			assert!(message.contains("--lossless"), "{message}");
			assert!(message.contains("WebP or PNG"), "{message}");
			assert!(message.contains(&format.to_string()), "{message}");
		}
	}

	/// AVIF can be translucent but not lossless, so a seam would change the
	/// heights. JPEG is never composited, so it loses nothing here.
	#[test]
	fn elevation_is_refused_only_where_a_seam_would_change_heights() {
		for format in [TileFormat::WEBP, TileFormat::PNG, TileFormat::JPG] {
			assert!(validate_encoding(format, false, true).is_ok(), "{format}");
		}
		let message = validate_encoding(TileFormat::AVIF, false, true)
			.unwrap_err()
			.to_string();
		assert!(message.contains("elevation") && message.contains("avif"), "{message}");
		assert!(message.contains("WebP or PNG"), "{message}");
	}

	/// The seam of two DEMs composites into an opaque tile, which images
	/// encode at --quality (#319). Heights are encoded without loss instead.
	#[test]
	fn elevation_tiles_are_encoded_without_loss() -> Result<()> {
		let config = AssembleConfig {
			elevation: true,
			..test_config()
		};
		let seam = composite_two_tiles(half_tile(true), half_tile(false), TileFormat::WEBP, true)?;
		let tiles = vec![(coord(0, 0, 0), seam), (coord(1, 0, 0), half_tile(true))];
		for result in encode_tiles_parallel(tiles, &config) {
			let (c, blob) = result?;
			assert_eq!(webp_chunk(&blob), *b"VP8L", "the tile at {c:?}");
		}
		Ok(())
	}

	/// A 16×16 tile as an input container holds it: an encoded blob, half
	/// opaque and half transparent, in `format` at `quality`.
	fn read_tile(format: TileFormat, quality: u8) -> Result<(Tile, Blob)> {
		let data = (0..16 * 16)
			.flat_map(|i: u32| {
				let [_, _, _, shade] = (i * 7).to_be_bytes();
				[shade, 255 - shade, 128, if i % 16 < 8 { 255 } else { 0 }]
			})
			.collect();
		encoded_tile(ImageBuffer::from_vec(16, 16, data).unwrap(), format, quality)
	}

	/// A 256×256 lossless WebP edge tile of photo-like noise, which a lossy
	/// encoder makes much smaller.
	fn read_noisy_tile() -> Result<(Tile, Blob)> {
		let mut state = 1u32;
		let data = (0..256 * 256)
			.flat_map(|i: u32| {
				state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
				let [red, green, blue, _] = state.to_be_bytes();
				[red, green, blue, if i % 256 < 128 { 255 } else { 0 }]
			})
			.collect();
		encoded_tile(ImageBuffer::from_vec(256, 256, data).unwrap(), TileFormat::WEBP, 100)
	}

	fn encoded_tile(
		image: ImageBuffer<versatiles_image::Rgba<u8>, Vec<u8>>,
		format: TileFormat,
		quality: u8,
	) -> Result<(Tile, Blob)> {
		let mut tile = Tile::from_image(DynamicImage::ImageRgba8(image), format)?;
		tile.change_format(format, Some(quality), None)?;
		let blob = tile.into_blob(&TileCompression::Uncompressed)?;
		Ok((
			Tile::from_blob(blob.clone(), TileCompression::Uncompressed, format),
			blob,
		))
	}

	fn encode_one(tile: Tile, config: &AssembleConfig) -> Result<Blob> {
		Ok(encode_tiles_parallel(vec![(coord(3, 1, 2), tile)], config).remove(0)?.1)
	}

	#[test]
	fn is_lossless_webp_reads_the_image_chunk() -> Result<()> {
		assert!(is_lossless_webp(&read_tile(TileFormat::WEBP, 100)?.1));
		let lossy = read_tile(TileFormat::WEBP, 70)?.1;
		assert_eq!(webp_chunk(&lossy), *b"VP8X", "lossy with alpha is an extended file");
		assert!(!is_lossless_webp(&lossy));
		assert!(!is_lossless_webp(&Blob::from(b"RIFF\0\0\0\0WEBP".to_vec())));
		assert!(!is_lossless_webp(&Blob::from(b"not a webp".to_vec())));
		Ok(())
	}

	/// A translucent tile only one input has is written as it was read,
	/// unless re-encoding is what makes it lossy.
	#[test]
	fn a_tile_only_one_input_has_keeps_its_encoding() -> Result<()> {
		let webp = test_config();
		let cases = [
			(
				"PNG",
				TileFormat::PNG,
				100,
				AssembleConfig {
					tile_format: TileFormat::PNG,
					..test_config()
				},
			),
			("lossy WebP", TileFormat::WEBP, 70, webp.clone()),
			(
				"lossless WebP with --lossless",
				TileFormat::WEBP,
				100,
				AssembleConfig {
					lossless: true,
					..webp.clone()
				},
			),
			(
				"lossless WebP of heights",
				TileFormat::WEBP,
				100,
				AssembleConfig {
					elevation: true,
					..webp.clone()
				},
			),
			(
				"lossless WebP at --quality 100",
				TileFormat::WEBP,
				100,
				AssembleConfig {
					quality: quality("100"),
					..webp.clone()
				},
			),
		];
		for (name, format, source_quality, config) in cases {
			let (tile, blob) = read_tile(format, source_quality)?;
			assert_eq!(encode_one(tile, &config)?, blob, "{name} is copied");
		}

		// What --quality is for by default: the lossless edge tiles `mosaic
		// tile` writes become lossy, and smaller.
		let (tile, blob) = read_noisy_tile()?;
		let encoded = encode_one(tile, &webp)?;
		assert!(!is_lossless_webp(&encoded), "lossless WebP is made lossy");
		assert!(
			encoded.len() < blob.len() / 2,
			"{} bytes from {}",
			encoded.len(),
			blob.len()
		);
		Ok(())
	}

	/// Where lossy WebP comes out larger than the lossless original, as on
	/// smooth tiles, the original is kept: better and smaller.
	#[test]
	fn a_lossless_tile_is_kept_where_lossy_would_be_larger() -> Result<()> {
		let (tile, blob) = read_tile(TileFormat::WEBP, 100)?;
		let mut lossy = Tile::from_blob(blob.clone(), TileCompression::Uncompressed, TileFormat::WEBP);
		lossy.change_format(TileFormat::WEBP, Some(75), None)?;
		let lossy = lossy.into_blob(&TileCompression::Uncompressed)?;
		assert!(
			lossy.len() > blob.len(),
			"the case needs lossy to be larger: {} vs {}",
			lossy.len(),
			blob.len()
		);

		assert_eq!(encode_one(tile, &test_config())?, blob);
		Ok(())
	}

	/// The output compression applies to a copied tile too.
	#[test]
	fn a_copied_tile_is_compressed_for_the_output() -> Result<()> {
		let config = AssembleConfig {
			tile_format: TileFormat::PNG,
			tile_compression: TileCompression::Gzip,
			..test_config()
		};
		let (tile, blob) = read_tile(TileFormat::PNG, 100)?;
		let written = encode_one(tile, &config)?;
		assert_eq!(
			versatiles_core::compression::decompress(written, &TileCompression::Gzip)?,
			blob
		);
		Ok(())
	}

	#[test]
	fn composite_transparent_over_transparent() -> Result<()> {
		let base = translucent_rgba_tile(0);
		let top = translucent_rgba_tile(0);
		let result = composite_two_tiles(base, top, TileFormat::WEBP, false)?;
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
		let composited = composite_two_tiles(base, top, TileFormat::WEBP, false)?;

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
		let config = AssembleConfig {
			quality: quality(""),
			lossless: false,
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
			elevation: false,
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
		let config = AssembleConfig {
			quality: quality("50,5:90,10:30"),
			lossless: false,
			tile_format: TileFormat::WEBP,
			tile_compression: TileCompression::Uncompressed,
			elevation: false,
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
				let matching =
					versatiles_container::TileSourceMetadata::new(fmt, comp, versatiles_container::Traversal::ANY, None);
				assert!(validate_source_format("test", &matching, &config).is_ok());

				// Mismatched format should fail
				let wrong_format = if fmt == TileFormat::PNG {
					TileFormat::WEBP
				} else {
					TileFormat::PNG
				};
				let wrong_fmt = versatiles_container::TileSourceMetadata::new(
					wrong_format,
					comp,
					versatiles_container::Traversal::ANY,
					None,
				);
				assert!(validate_source_format("test", &wrong_fmt, &config).is_err());
			}
		}
	}
}
