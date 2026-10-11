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
	pub fn new(
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
