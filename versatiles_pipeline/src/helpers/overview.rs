use anyhow::{Result, ensure};
use dashmap::DashMap;
use imageproc::image::{DynamicImage, GenericImage};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use versatiles_container::{Tile, TileSource, TileSourceMetadata, Traversal, TraversalOrder};
use versatiles_core::{Blob, TileBBox, TileBBoxMap, TileCoord, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitInfo;

static BLOCK_TILE_COUNT: u32 = 16;

/// Scaling function type used to downscale tiles by a factor of 2.
pub type ScaleDownFn = Arc<dyn Fn(&DynamicImage) -> Result<DynamicImage> + Send + Sync>;

/// Shared overview core that generates lower-zoom tiles by downscaling from a base zoom level.
///
/// The actual downscaling algorithm is provided via `scale_fn`, allowing reuse for both
/// standard raster (channel-wise averaging) and DEM (24-bit raw value averaging) tiles.
pub struct OverviewCore {
	pub metadata: TileSourceMetadata,
	pub source: Box<dyn TileSource>,
	pub tilejson: TileJSON,
	pub level_base: u8,
	pub tile_size: u32,
	#[allow(clippy::type_complexity)]
	pub cache: Arc<DashMap<TileCoord, Vec<(TileCoord, Option<Blob>)>>>,
	pub(crate) cache_bytes: Arc<AtomicUsize>,
	scale_fn: ScaleDownFn,
}

impl OverviewCore {
	/// Build an overview core from a tile source.
	///
	/// - `level`: zoom level to build the overview from (defaults to source max zoom)
	/// - `scale_fn`: function that downscales an image by factor 2
	///
	/// The tile size is read from the source's TileJSON (defaults to 512 if not set).
	pub fn new(source: Box<dyn TileSource>, level: Option<u8>, scale_fn: ScaleDownFn) -> Result<Self> {
		ensure!(source.metadata().traversal.is_any());

		let mut metadata = source.metadata().clone();
		let mut tilejson = source.tilejson().clone();

		let level_base = level.unwrap_or_else(|| source.metadata().bbox_pyramid.level_max().unwrap());

		if let Some(mut level_bbox) = metadata.bbox_pyramid.level(level_base).bbox() {
			while level_bbox.level() > 0 {
				level_bbox.level_down();
				metadata.bbox_pyramid.insert_bbox(&level_bbox)?;
			}
		}
		metadata.update_tilejson(&mut tilejson);

		let tile_size = tilejson.tile_size.map_or(512, |ts| u32::from(ts.size()));
		let cache = Arc::new(DashMap::new());
		let cache_bytes = Arc::new(AtomicUsize::new(0));
		metadata.traversal = Traversal::new(TraversalOrder::DepthFirst, BLOCK_TILE_COUNT, BLOCK_TILE_COUNT)?;

		Ok(Self {
			metadata,
			source,
			tilejson,
			level_base,
			tile_size,
			cache,
			cache_bytes,
			scale_fn,
		})
	}

	#[context("Failed to build images from cache for bbox {bbox:?}")]
	pub async fn build_images_from_cache(&self, bbox: TileBBox) -> Result<TileBBoxMap<Option<DynamicImage>>> {
		log::trace!("build_images_from_cache: {bbox:?}");

		let size = bbox.max_count().min(BLOCK_TILE_COUNT);

		ensure!(bbox.level() < self.level_base, "Invalid level");
		ensure!(bbox.width() <= size, "Invalid width");
		ensure!(bbox.height() <= size, "Invalid height");

		let bbox0 = bbox.rounded(size);
		assert_eq!(bbox0.width(), size);
		assert_eq!(bbox0.height(), size);

		let mut map: TileBBoxMap<Vec<(TileCoord, DynamicImage)>> = TileBBoxMap::new_default(bbox)?;

		let full_size = self.tile_size;
		let half_size = self.tile_size / 2;

		// get images from cache
		for q in &[0, 1, 2, 3] {
			let bbox1 = bbox0.leveled_up().quadrant(*q)?;

			if let Some((_, blobs1)) = self.cache.remove(&bbox1.min_tile()?) {
				let entry_bytes = estimate_entry_bytes(&blobs1);
				self.cache_bytes.fetch_sub(entry_bytes, Ordering::Relaxed);
				for (coord1, blob1) in blobs1 {
					if let Some(blob1) = blob1 {
						let image1 = versatiles_image::format::png::blob2image(&blob1)?;
						assert_eq!(image1.width(), half_size);
						assert_eq!(image1.height(), half_size);
						let coord0 = coord1.to_level_decreased()?;
						if bbox.includes_coord(&coord0)? {
							map.get_mut(&coord0)?.push((coord1, image1));
						}
					}
				}
			}
		}

		let vec: Vec<(TileCoord, DynamicImage)> =
			futures::future::join_all(map.into_iter().filter(|(_, sub_entries)| !sub_entries.is_empty()).map(
				|(coord0, sub_entries)| {
					tokio::task::spawn_blocking(move || {
						let mut image_tmp = DynamicImage::new_rgba8(full_size, full_size);
						for (coord1, image1) in sub_entries {
							image_tmp
								.copy_from(&image1, (coord1.x % 2) * half_size, (coord1.y % 2) * half_size)
								.unwrap();
						}
						image_tmp.into_optional().map(|image| (coord0, image))
					})
				},
			))
			.await
			.into_iter()
			.filter_map(|r| r.unwrap())
			.collect();

		TileBBoxMap::<Option<DynamicImage>>::from_iter(bbox, vec.into_iter())
	}

	/// Consume the container: scale each image down for the cache and wrap
	/// the original in a [`Tile`] — all in parallel, with zero image clones.
	#[context("Failed to scale and encode tiles for bbox {bbox:?}")]
	async fn scale_cache_and_encode(
		&self,
		container: TileBBoxMap<Option<DynamicImage>>,
		bbox: TileBBox,
	) -> Result<Vec<(TileCoord, Tile)>> {
		let container_bbox = *container.bbox();
		let format = self.source.metadata().tile_format;
		let full_size = self.tile_size;
		let scale_fn = self.scale_fn.clone();
		let need_cache = container_bbox.level() > 0 && container_bbox.level() <= self.level_base;

		let results: Vec<_> = futures::future::join_all(container.into_iter().map(|(coord, image_opt)| {
			let scale_fn = scale_fn.clone();
			tokio::task::spawn_blocking(move || {
				let cached = if need_cache {
					image_opt.as_ref().map(|img| {
						assert_eq!(img.width(), full_size);
						assert_eq!(img.height(), full_size);
						let scaled = scale_fn(img).unwrap();
						versatiles_image::format::png::encode(&scaled, Some(0)).unwrap()
					})
				} else {
					None
				};
				let tile = if bbox.includes_coord(&coord).unwrap() {
					image_opt.map(|img| (coord, Tile::from_image(img, format).unwrap()))
				} else {
					None
				};
				((coord, cached), tile)
			})
		}))
		.await
		.into_iter()
		.collect::<Result<Vec<_>, _>>()?;

		let (cache_entries, tiles): (Vec<_>, Vec<_>) = results.into_iter().unzip();

		if need_cache {
			let mut key = container_bbox.min_tile()?;
			key.floor(BLOCK_TILE_COUNT);
			let entry_bytes = estimate_entry_bytes(&cache_entries);
			let total = self.cache_bytes.fetch_add(entry_bytes, Ordering::Relaxed) + entry_bytes;
			let gb = total as f64 / (1024.0 * 1024.0 * 1024.0);
			if gb > 2.0 {
				log::warn!("Overview staging area using {gb:.1} GB — consider reducing dataset size or base zoom level");
			}
			self.cache.insert(key, cache_entries);
		}

		Ok(tiles.into_iter().flatten().collect())
	}

	pub async fn get_tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		if bbox.level() >= self.level_base {
			return self.source.get_tile_coord_stream(bbox).await;
		}

		let mut source_bbox = bbox.at_level(self.level_base);
		source_bbox.intersect_with_pyramid(&self.metadata.bbox_pyramid);
		if source_bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		let mut coords = std::collections::HashSet::new();
		let mut stream = self.source.get_tile_coord_stream(source_bbox).await?;
		while let Some((coord, _)) = stream.next().await {
			let c = coord.at_level(bbox.level());
			if bbox.includes_coord(&c)? {
				coords.insert(c);
			}
		}

		let vec: Vec<(TileCoord, ())> = coords.into_iter().map(|c| (c, ())).collect();
		Ok(TileStream::from_vec(vec))
	}

	#[context("Failed to get stream for bbox: {:?}", bbox)]
	pub async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("overview::get_tile_stream {bbox:?}");

		if bbox.level() > self.level_base {
			return self.source.get_tile_stream(bbox).await;
		}

		let size = bbox.max_count().min(BLOCK_TILE_COUNT);
		let mut bbox0 = bbox.rounded(size);
		assert_eq!(bbox0.width(), size);
		assert_eq!(bbox0.height(), size);
		bbox0.intersect_with_pyramid(&self.metadata.bbox_pyramid);

		let container: TileBBoxMap<Option<DynamicImage>> = if bbox.level() == self.level_base {
			log::trace!("Fetching images from source for bbox {bbox:?}");
			TileBBoxMap::<Option<DynamicImage>>::from_stream(
				bbox,
				self
					.source
					.get_tile_stream(bbox)
					.await?
					.map_parallel_try(|_coord, tile| tile.into_image())
					.unwrap_results(),
			)
			.await?
		} else {
			log::trace!("Building images from cache for bbox {bbox:?}");
			self.build_images_from_cache(bbox0).await?
		};

		log::trace!("Scaling, caching, and encoding tiles for bbox {bbox:?}");
		let vec = self.scale_cache_and_encode(container, bbox).await?;

		Ok(TileStream::from_vec(vec))
	}
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn estimate_entry_bytes(entries: &[(TileCoord, Option<Blob>)]) -> usize {
	entries
		.iter()
		.map(|(_, blob)| blob.as_ref().map_or(16, |b| b.len() as usize))
		.sum()
}

impl std::fmt::Debug for OverviewCore {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("OverviewCore")
			.field("level_base", &self.level_base)
			.field("tile_size", &self.tile_size)
			.field("metadata", &self.metadata)
			.finish()
	}
}
