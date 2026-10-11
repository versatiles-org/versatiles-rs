//! Overscaling from one fixed level: the behaviour `raster_overscale` had
//! before it filled gaps, kept for `from_stacked_raster`.

use std::{fmt::Debug, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use moka::future::Cache;
use versatiles_container::{SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::DynamicImage;

use super::scale::Scaling;

/// Serves every level above `level_base` from the tiles of that one level.
///
/// Wraps a source tile provider and intercepts tile requests at zoom levels
/// above `level_base`, generating synthetic tiles by extracting and upscaling
/// from the base level tiles. Tiles at `level_base` and below are passed
/// through; a tile missing there stays missing.
#[derive(Clone)]
pub struct FixedOverscale {
	metadata: TileSourceMetadata,
	source: SharedTileSource,
	tilejson: TileJSON,
	/// The zoom level used as source for overscaled tiles
	level_base: u8,
	/// The minimum zoom level of the source (used as climbing stop)
	level_min: u8,
	/// Whether to climb to parent tiles when source tile is missing
	enable_climbing: bool,
	/// How a region of a source tile is turned into a full tile
	scaling: Scaling,
	/// LRU cache of decoded source images to avoid redundant decoding
	cache: Arc<Cache<TileCoord, Option<Arc<DynamicImage>>>>,
}

impl Debug for FixedOverscale {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FixedOverscale")
			.field("metadata", &self.metadata)
			.field("source", &self.source)
			.field("tilejson", &self.tilejson)
			.field("level_base", &self.level_base)
			.field("level_min", &self.level_min)
			.field("enable_climbing", &self.enable_climbing)
			.field("scaling", &self.scaling)
			.field("cache", &"<moka::future::Cache>")
			.finish()
	}
}

impl FixedOverscale {
	/// The setup `from_stacked_raster auto_overscale=true` wraps each of its
	/// sources in: up to `native_level_max` the source's own tiles, above it
	/// upscales of that level, or of the nearest lower one where it has a hole.
	///
	/// Kept apart from what `raster_overscale` does in a pipeline, so that the
	/// tiles a stack is blended from do not change when that does (#284).
	pub fn for_stacking(source: Box<dyn TileSource>, native_level_max: u8, level_max: u8) -> Result<Self> {
		Self::new(source, Some(native_level_max), Some(level_max), true)
	}

	/// `level_base` defaults to the source's highest level, `level_max` to the
	/// highest level there is.
	fn new(
		source: Box<dyn TileSource>,
		level_base: Option<u8>,
		level_max: Option<u8>,
		enable_climbing: bool,
	) -> Result<Self> {
		let mut metadata = source.as_ref().metadata().clone();

		let source_pyramid = metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow::anyhow!("source tile_pyramid not set"))?;

		let level_base = match level_base {
			Some(level) => level,
			None => source_pyramid
				.level_max()
				.ok_or_else(|| anyhow::anyhow!("source pyramid is empty"))?,
		};
		log::trace!("level_base {level_base}");

		let level_max = level_max.unwrap_or(MAX_ZOOM_LEVEL).clamp(level_base, MAX_ZOOM_LEVEL);

		let mut new_pyramid = source_pyramid.as_ref().clone();
		let mut level_bbox = new_pyramid.level_ref(level_base).to_bbox();
		while !level_bbox.is_empty() && level_bbox.level() < level_max {
			level_bbox.level_up();
			new_pyramid.insert_bbox(&level_bbox)?;
		}
		let level_min = new_pyramid.level_min().unwrap_or(0);
		metadata.set_tile_pyramid(new_pyramid);

		let mut tilejson = source.as_ref().tilejson().clone();
		metadata.update_tilejson(&mut tilejson);
		let cache = Cache::builder()
			.max_capacity(512 * 1024 * 1024) // 512MB limit
			.weigher(|_k: &TileCoord, v: &Option<Arc<DynamicImage>>| -> u32 {
				v.as_ref().map_or(8, |image| image.width() * image.height() * 4) // RGBA bytes
			})
			.build();
		let cache = Arc::new(cache);

		Ok(Self {
			metadata,
			source: Arc::from(source),
			tilejson,
			level_base,
			level_min,
			cache,
			enable_climbing,
			scaling: Scaling::Color,
		})
	}

	/// Finds a source tile that covers the requested destination coordinate.
	///
	/// This method implements the core tile lookup logic:
	/// 1. Calculates which source tile at `level_base` contains the requested tile
	/// 2. Attempts to fetch that tile
	/// 3. If `with_climbing` is enabled and the tile is missing, recursively tries
	///    parent tiles at lower zoom levels until one is found or `level_min` is reached
	///
	/// Returns the source tile's coordinate and image, which can then be used by
	/// `extract_image` to generate the actual output tile.
	#[context("finding tile for coord {:?}", coord_dst)]
	async fn find_tile(
		&self,
		coord_dst: TileCoord,
		with_climbing: bool,
	) -> Result<Option<(TileCoord, Arc<DynamicImage>)>> {
		let mut coord_src = coord_dst.at_level(self.level_base.min(coord_dst.level));

		if with_climbing {
			// Tile climbing: try progressively lower zoom levels until we find a tile
			loop {
				if let Some(image) = self.try_fetch_tile(coord_src).await? {
					return Ok(Some((coord_src, image)));
				}

				// Climb to parent tile (lower zoom level, covers 4x the area)
				if coord_src.level <= self.level_min {
					return Ok(None);
				}
				coord_src = coord_src.to_level_decreased()?;
			}
		}
		// No climbing: single attempt at the calculated source coordinate
		if let Some(image) = self.try_fetch_tile(coord_src).await? {
			return Ok(Some((coord_src, image)));
		}
		Ok(None)
	}

	/// Attempts to fetch a tile at the given coordinate, checking cache first.
	/// Returns None if the tile doesn't exist at this coordinate.
	async fn try_fetch_tile(&self, coord: TileCoord) -> Result<Option<Arc<DynamicImage>>> {
		// Check cache - no lock needed!
		if let Some(cached_image) = self.cache.get(&coord).await {
			return Ok(cached_image);
		}

		// Fetch from source
		let image = self
			.source
			.tile(&coord)
			.await?
			.map(|t| t.into_image().map(Arc::new))
			.transpose()?;

		self.cache.insert(coord, image.clone()).await;
		Ok(image)
	}
}

#[async_trait]
impl TileSource for FixedOverscale {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_overscale", self.source.as_ref().source_type())
	}

	#[context("Failed to get stream for bbox: {:?}", bbox_dst)]
	async fn tile_stream(&self, bbox_dst: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("raster_overscale::tile_stream {bbox_dst:?}");

		if let Some(pyramid) = self.metadata.tile_pyramid()
			&& !pyramid.intersects_bbox(&bbox_dst)
		{
			log::trace!("tile_stream outside tile_pyramid");
			return Ok(TileStream::empty());
		}

		if bbox_dst.level() <= self.level_base {
			log::trace!("tile_stream level <= level_base");
			return self.source.as_ref().tile_stream(bbox_dst).await;
		}

		// Use tile climbing for all upscaling - process in parallel
		let self_arc = Arc::new(self.clone()); // Share Operation across tasks
		let enable_climbing = self.enable_climbing;
		let tile_format = *self.metadata.tile_format();
		let scaling = self.scaling;

		let stream = TileStream::from_bbox_async_parallel(bbox_dst, move |coord_dst| {
			let self_arc = self_arc.clone();
			async move {
				match self_arc.find_tile(coord_dst, enable_climbing).await {
					Ok(Some((coord_src, image_src))) => match scaling.extract(&image_src, coord_src, coord_dst) {
						Ok(img) => match Tile::from_image(img, tile_format) {
							Ok(tile) => Some((coord_dst, tile)),
							Err(e) => {
								log::error!("Error creating tile {coord_dst:?}: {e:?}");
								None
							}
						},
						Err(e) => {
							log::error!("Error extracting image {coord_dst:?}: {e:?}");
							None
						}
					},
					Ok(None) => None,
					Err(e) => {
						log::error!("Error processing tile {coord_dst:?}: {e:?}");
						None
					}
				}
			}
		});

		Ok(stream)
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		if let Some(pyramid) = self.metadata.tile_pyramid()
			&& !pyramid.intersects_bbox(&bbox)
		{
			return Ok(TileStream::empty());
		}
		if bbox.level() <= self.level_base {
			return self.source.as_ref().tile_coord_stream(bbox).await;
		}

		// Above level_base: a tile exists if a source tile covers it.
		// Query source coords and map each up to the requested level.
		let mut coords = std::collections::HashSet::new();

		// Without climbing, only level_base is checked.
		// With climbing, check from level_base down to level_min.
		let level_stop = if self.enable_climbing {
			self.level_min
		} else {
			self.level_base
		};

		for src_level in (level_stop..=self.level_base).rev() {
			let source_bbox = bbox.at_level(src_level);
			let mut stream = self.source.as_ref().tile_coord_stream(source_bbox).await?;
			while let Some((src_coord, _)) = stream.next().await {
				// Each source coord at src_level covers a block of output coords
				let scale = 1u32 << (bbox.level() - src_coord.level);
				let base_x = src_coord.x * scale;
				let base_y = src_coord.y * scale;
				for dy in 0..scale {
					for dx in 0..scale {
						let coord = TileCoord::new(bbox.level(), base_x + dx, base_y + dy)?;
						if bbox.includes_coord(&coord) {
							coords.insert(coord);
						}
					}
				}
			}
		}

		let vec: Vec<(TileCoord, ())> = coords.into_iter().map(|c| (c, ())).collect();
		Ok(TileStream::from_vec(vec))
	}
}

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test data is built from literal values")]
mod tests {
	use rstest::rstest;
	use versatiles_container::Tile;
	use versatiles_core::{TileFormat, TilePyramid};
	use versatiles_image::{DynamicImage, DynamicImageTraitConvert, DynamicImageTraitOperation};

	use super::*;
	use crate::helpers::dummy_image_source::DummyImageSource;

	fn make_gradient_image(channel_count: usize) -> DynamicImage {
		let s = 256;
		match channel_count {
			1 => DynamicImage::from_fn(s, s, |x, _| [x as u8]),
			2 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8]),
			3 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8, 255 - x as u8]),
			4 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8, 255 - x as u8, 255 - y as u8]),
			_ => panic!("unsupported channel count {channel_count}"),
		}
	}

	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "`c` is a u8 and `scale` a small u32, so the average is 0..=255"
	)]
	async fn get_avg(op: &FixedOverscale, coord: (u8, u8, u8), scale: u32) -> Vec<u8> {
		let (level, x, y) = coord;
		let coord = TileCoord::new(level, u32::from(x), u32::from(y))
			.unwrap()
			.to_tile_bbox();
		let mut tiles = op.tile_stream(coord).await.unwrap().to_vec().await;
		assert_eq!(tiles.len(), 1);
		let mut tile = tiles.pop().unwrap().1;
		let image = tile.as_image().unwrap();
		let avg = image.average_color();
		avg.into_iter()
			.map(|c| (f64::from(c) / f64::from(scale)).round() as u8)
			.collect()
	}

	fn build_op(channel_count: usize) -> Result<FixedOverscale> {
		let image = make_gradient_image(channel_count);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		FixedOverscale::new(source, Some(2), None, false)
	}

	#[rstest]
	#[case::l(1,[vec![16], vec![48], vec![16], vec![48]])]
	#[case::la(2,[vec![16,16], vec![48,16], vec![16,48], vec![48,48]])]
	#[case::rgb(3,[vec![16,16,48], vec![48,16,16], vec![16,48,48], vec![48,48,16]])]
	#[case::rgba(4,[vec![16,16,48,48], vec![48,16,16,48], vec![16,48,48,16], vec![48,48,16,16]])]
	#[tokio::test]
	async fn overscale_to_z3(#[case] channel_count: usize, #[case] expected: [Vec<u8>; 4]) {
		let op = build_op(channel_count).unwrap();

		let avg_colors = [
			get_avg(&op, (3, 2, 2), 4).await,
			get_avg(&op, (3, 3, 2), 4).await,
			get_avg(&op, (3, 2, 3), 4).await,
			get_avg(&op, (3, 3, 3), 4).await,
		];

		assert_eq!(avg_colors, expected);
	}

	#[tokio::test]
	async fn overscale_to_z4_rgb() -> Result<()> {
		let op = build_op(3)?;

		for x in 0..4 {
			for y in 0..4 {
				assert_eq!(
					get_avg(&op, (4, 4 + x, 4 + y), 32).await,
					vec![1 + x * 2, 1 + y * 2, 7 - x * 2]
				);
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_overscale_without_climbing() -> Result<()> {
		let image = make_gradient_image(3);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		// Build with climbing disabled
		let op = FixedOverscale::new(source, Some(2), None, false)?;

		// Should work for tiles at level_base (z=2)
		let coord_base = TileCoord::new(2, 0, 0)?.to_tile_bbox();
		let tiles = op.tile_stream(coord_base).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should return tile at level_base");

		// Should work for tiles above level_base (extracted from level_base)
		let coord_high = TileCoord::new(3, 0, 0)?.to_tile_bbox();
		let tiles = op.tile_stream(coord_high).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should extract from level_base for high zoom");

		// Multiple high-zoom tiles should reuse cached base tile
		let coord_high2 = TileCoord::new(3, 1, 0)?.to_tile_bbox();
		let tiles2 = op.tile_stream(coord_high2).await?.to_vec().await;
		assert_eq!(tiles2.len(), 1, "Should also work for adjacent tile");

		Ok(())
	}

	#[tokio::test]
	async fn test_overscale_with_climbing_enabled() -> Result<()> {
		let image = make_gradient_image(3);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		// Build with climbing ENABLED
		let op = FixedOverscale::new(source, Some(2), None, true)?;

		// Should work with climbing enabled
		let coord = TileCoord::new(3, 0, 0)?.to_tile_bbox();
		let tiles = op.tile_stream(coord).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should return tile with climbing enabled");

		Ok(())
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let op = build_op(3)?;
		let source_type = op.source_type();
		assert!(source_type.to_string().contains("raster_overscale"));
		Ok(())
	}

	#[tokio::test]
	async fn test_metadata_and_tilejson() -> Result<()> {
		let op = build_op(3)?;
		// metadata and tilejson should be available
		let metadata = op.metadata();
		assert!(metadata.tile_pyramid().unwrap().level_max().is_some());
		let _tilejson = op.tilejson();
		Ok(())
	}

	#[tokio::test]
	async fn test_level_max_parameter() -> Result<()> {
		let image = make_gradient_image(3);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		// Build with custom level_max
		let op = FixedOverscale::new(source, Some(2), Some(5), false)?;

		// Should have extended max level
		let metadata = op.metadata();
		assert!(metadata.tile_pyramid().unwrap().level_max().unwrap() >= 5);
		Ok(())
	}

	#[tokio::test]
	async fn test_level_below_base_passthrough() -> Result<()> {
		let op = build_op(3)?;

		// Request at level below level_base should pass through to source
		let coord = TileCoord::new(2, 1, 1)?.to_tile_bbox();
		let tiles = op.tile_stream(coord).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should passthrough to source for level <= level_base");
		Ok(())
	}

	// --- the mode `from_stacked_raster auto_overscale=true` relies on ---

	/// A source with holes: levels 1 to 4, not full, and inside its pyramid a
	/// tile missing here and there. Every tile has its own gradient, so both
	/// the tile an upscale came from and the region cut out of it show.
	fn sparse_source() -> Result<DummyImageSource> {
		let mut pyramid = TilePyramid::new_empty();
		pyramid.insert_bbox(&TileBBox::new_full(1)?)?;
		pyramid.insert_bbox(&TileBBox::from_min_and_max(2, 0, 0, 3, 2)?)?;
		pyramid.insert_bbox(&TileBBox::from_min_and_max(3, 1, 1, 6, 5)?)?;
		pyramid.insert_bbox(&TileBBox::from_min_and_max(4, 2, 2, 9, 9)?)?;

		DummyImageSource::new(
			|coord| {
				let (level, x, y) = (u32::from(coord.level), coord.x, coord.y);
				if (x + 2 * y + level) % 5 == 0 {
					return None;
				}
				let image = DynamicImage::from_fn(8, 8, |px, py| {
					[
						(level * 37 + px * 3) as u8,
						(x * 29 + py * 5) as u8,
						(y * 31 + px + py) as u8,
					]
				});
				Some(Tile::from_image(image, TileFormat::PNG).unwrap())
			},
			TileFormat::PNG,
			Some(pyramid),
		)
	}

	/// One line per level: how many tiles and coordinates the operation hands
	/// out, how many its pyramid announces, and a hash over which tiles they
	/// are and what is in them.
	async fn fingerprint(op: &dyn TileSource, level_max: u8) -> Result<String> {
		fn fnv(hash: &mut u64, bytes: &[u8]) {
			for byte in bytes {
				*hash ^= u64::from(*byte);
				*hash = hash.wrapping_mul(0x0100_0000_01b3);
			}
		}
		let key = |c: &TileCoord| (c.level, c.x, c.y);

		let pyramid = op.metadata().tile_pyramid().unwrap();
		let mut lines = Vec::new();
		for level in 0..=level_max {
			let bbox = TileBBox::new_full(level)?;
			let mut hash = 0xcbf2_9ce4_8422_2325_u64;

			let mut tiles = op.tile_stream(bbox).await?.to_vec().await;
			tiles.sort_by_key(|(coord, _)| key(coord));
			let tile_count = tiles.len();
			for (coord, tile) in tiles {
				fnv(&mut hash, format!("{:?}", key(&coord)).as_bytes());
				fnv(&mut hash, tile.into_image()?.as_bytes());
			}

			let mut coords: Vec<TileCoord> = op
				.tile_coord_stream(bbox)
				.await?
				.to_vec()
				.await
				.into_iter()
				.map(|(coord, ())| coord)
				.collect();
			coords.sort_by_key(key);
			for coord in &coords {
				fnv(&mut hash, format!("{:?}", key(coord)).as_bytes());
			}

			lines.push(format!(
				"level {level}: tiles={tile_count} coords={} pyramid={} hash={hash:016x}",
				coords.len(),
				pyramid.level_ref(level).count_tiles()
			));
		}
		Ok(lines.join("\n"))
	}

	/// Frozen before `raster_overscale` learned to fill gaps (#284), so that
	/// nothing in that work changes what a stack of sources is blended from.
	#[tokio::test]
	async fn the_stacking_mode_gives_what_it_always_gave() -> Result<()> {
		let op = FixedOverscale::for_stacking(Box::new(sparse_source()?), 4, 7)?;

		assert_eq!(
			fingerprint(&op, 8).await?,
			[
				"level 0: tiles=0 coords=0 pyramid=0 hash=cbf29ce484222325",
				"level 1: tiles=4 coords=4 pyramid=4 hash=c0bf84935b355a01",
				"level 2: tiles=10 coords=12 pyramid=12 hash=fd3cd4d04f3db28c",
				"level 3: tiles=24 coords=30 pyramid=30 hash=2b65ddbf30192d40",
				"level 4: tiles=51 coords=64 pyramid=64 hash=f0218e8b73d4abe5",
				"level 5: tiles=1024 coords=1024 pyramid=256 hash=b29d7d012c2f28c8",
				"level 6: tiles=4096 coords=4096 pyramid=1024 hash=da95c98490faa769",
				"level 7: tiles=16384 coords=16384 pyramid=4096 hash=96aa0f8a1f9d18bf",
				"level 8: tiles=0 coords=0 pyramid=0 hash=cbf29ce484222325",
			]
			.join("\n")
		);
		Ok(())
	}
}
