use anyhow::{Result, bail, ensure};
use dashmap::DashMap;
use imageproc::image::{DynamicImage, GenericImage};
use std::sync::Arc;
use versatiles_container::{Tile, TileSource, TileSourceMetadata, Traversal, TraversalOrder};
use versatiles_core::{TileBBox, TileBBoxMap, TileCoord, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitInfo;

static BLOCK_TILE_COUNT: u32 = 32;

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
	pub cache: Arc<DashMap<TileCoord, Vec<(TileCoord, Option<DynamicImage>)>>>,
	scale_fn: ScaleDownFn,
}

impl OverviewCore {
	/// Build an overview core from a tile source.
	///
	/// - `level`: zoom level to build the overview from (defaults to source max zoom)
	/// - `tile_size`: pixel size of tiles (defaults to 512)
	/// - `scale_fn`: function that downscales an image by factor 2
	pub fn new(
		source: Box<dyn TileSource>,
		level: Option<u8>,
		tile_size: Option<u32>,
		scale_fn: ScaleDownFn,
	) -> Result<Self> {
		ensure!(source.metadata().traversal.is_any());

		let mut metadata = source.metadata().clone();

		let level_base = level.unwrap_or_else(|| source.metadata().bbox_pyramid.get_level_max().unwrap());

		let mut level_bbox = *metadata.bbox_pyramid.get_level_bbox(level_base);
		while level_bbox.level > 0 {
			level_bbox.level_down();
			metadata.bbox_pyramid.set_level_bbox(level_bbox);
		}

		let mut tilejson = source.tilejson().clone();
		metadata.update_tilejson(&mut tilejson);

		let tile_size = tile_size.unwrap_or(512);
		let cache = Arc::new(DashMap::new());
		metadata.traversal = Traversal::new(TraversalOrder::DepthFirst, BLOCK_TILE_COUNT, BLOCK_TILE_COUNT)?;

		Ok(Self {
			metadata,
			source,
			tilejson,
			level_base,
			tile_size,
			cache,
			scale_fn,
		})
	}

	#[context("Failed to add images to cache from container {container:?}")]
	pub async fn add_images_to_cache(&self, container: &TileBBoxMap<Option<DynamicImage>>) -> Result<()> {
		log::trace!("add_images_to_cache: {:?}", container.bbox());

		let bbox = container.bbox();
		if bbox.level == 0 || bbox.level > self.level_base {
			return Ok(());
		}

		if bbox.width() > BLOCK_TILE_COUNT || bbox.height() > BLOCK_TILE_COUNT {
			bail!("Container bbox is too large: {bbox:?}");
		}

		let full_size = self.tile_size;
		let scale_fn = self.scale_fn.clone();

		let images: Vec<(TileCoord, Option<DynamicImage>)> =
			futures::future::join_all(container.iter().map(|(coord, item)| {
				let item = item.clone();
				let scale_fn = scale_fn.clone();
				tokio::task::spawn_blocking(move || {
					if let Some(image) = item {
						assert_eq!(image.width(), full_size);
						assert_eq!(image.height(), full_size);
						(coord, Some(scale_fn(&image).unwrap()))
					} else {
						(coord, None)
					}
				})
			}))
			.await
			.into_iter()
			.collect::<Result<Vec<_>, _>>()?;

		let mut coord = bbox.min_corner()?;
		coord.floor(BLOCK_TILE_COUNT);
		self.cache.insert(coord, images);

		Ok(())
	}

	#[context("Failed to build images from cache for bbox {bbox:?}")]
	pub async fn build_images_from_cache(&self, bbox: TileBBox) -> Result<TileBBoxMap<Option<DynamicImage>>> {
		log::trace!("build_images_from_cache: {bbox:?}");

		let size = bbox.max_count().min(BLOCK_TILE_COUNT);

		ensure!(bbox.level < self.level_base, "Invalid level");
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
			let bbox1 = bbox0.leveled_up().get_quadrant(*q)?;

			if let Some((_key, images1)) = self.cache.remove(&bbox1.min_corner()?) {
				for (coord1, image1) in images1 {
					if let Some(image1) = image1 {
						assert_eq!(image1.width(), half_size);
						assert_eq!(image1.height(), half_size);
						let coord0 = coord1.to_level_decreased()?;
						if bbox.contains(&coord0) {
							map.get_mut(&coord0)?.push((coord1, image1));
						}
					}
				}
			}
		}

		let vec: Vec<(TileCoord, DynamicImage)> = map
			.into_iter()
			.filter_map(|(coord0, sub_entries)| {
				if sub_entries.is_empty() {
					return None;
				}

				let mut image_tmp = DynamicImage::new_rgba8(full_size, full_size);
				for (coord1, image1) in sub_entries {
					image_tmp
						.copy_from(&image1, (coord1.x % 2) * half_size, (coord1.y % 2) * half_size)
						.unwrap();
				}

				image_tmp.into_optional().map(|image| (coord0, image))
			})
			.collect();

		TileBBoxMap::<Option<DynamicImage>>::from_iter(bbox, vec.into_iter())
	}

	#[context("Failed to get stream for bbox: {:?}", bbox)]
	pub async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::debug!("get_tile_stream {bbox:?}");

		if bbox.level > self.level_base {
			return self.source.get_tile_stream(bbox).await;
		}

		let size = bbox.max_count().min(BLOCK_TILE_COUNT);
		let mut bbox0 = bbox.rounded(size);
		assert_eq!(bbox0.width(), size);
		assert_eq!(bbox0.height(), size);
		bbox0.intersect_with_pyramid(&self.metadata.bbox_pyramid);

		let container: TileBBoxMap<Option<DynamicImage>> = if bbox.level == self.level_base {
			log::trace!("Fetching images from source for bbox {bbox:?}");
			TileBBoxMap::<Option<DynamicImage>>::from_stream(
				bbox,
				self
					.source
					.get_tile_stream(bbox)
					.await?
					.map_parallel_try(|_coord, tile| versatiles_container::Tile::into_image(tile))
					.unwrap_results(),
			)
			.await?
		} else {
			log::trace!("Building images from cache for bbox {bbox:?}");
			self.build_images_from_cache(bbox0).await?
		};

		log::trace!("Adding images to cache for bbox {:?}", container.bbox());
		self.add_images_to_cache(&container).await?;

		let format = self.source.metadata().tile_format;

		log::trace!("Composing final stream for bbox {bbox:?}");
		let vec = container
			.into_iter()
			.filter_map(move |(c, o)| {
				if let Some(image) = o {
					if bbox.contains(&c) {
						Some((c, Tile::from_image(image, format).unwrap()))
					} else {
						None
					}
				} else {
					None
				}
			})
			.collect();

		Ok(TileStream::from_vec(vec))
	}
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
