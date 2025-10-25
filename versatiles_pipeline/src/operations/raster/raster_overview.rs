use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::{DynamicImage, GenericImage};
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use versatiles_container::Tile;
use versatiles_core::{cache::CacheMap, *};
use versatiles_derive::context;
use versatiles_image::traits::*;

static BLOCK_TILE_COUNT: u32 = 32;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// use this zoom level to build the overview. Defaults to the maximum zoom level of the source.
	level: Option<u8>,
	/// Size of the tiles in pixels. Defaults to 512.
	tile_size: Option<u32>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	source: Box<dyn OperationTrait>,
	tilejson: TileJSON,
	level_base: u8,
	tile_size: u32,
	traversal: Traversal,
	#[allow(clippy::type_complexity)]
	cache: Arc<Mutex<CacheMap<TileCoord, (TileCoord, Option<DynamicImage>)>>>,
}

impl Operation {
	fn build(
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>, anyhow::Error>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			ensure!(source.traversal().is_any());

			let mut parameters = source.parameters().clone();

			let level_base = args
				.level
				.unwrap_or_else(|| source.parameters().bbox_pyramid.get_level_max().unwrap());

			let mut level_bbox = *parameters.bbox_pyramid.get_level_bbox(level_base);
			while level_bbox.level > 0 {
				level_bbox.level_down();
				parameters.bbox_pyramid.set_level_bbox(level_bbox);
			}

			let mut tilejson = source.tilejson().clone();
			tilejson.update_from_reader_parameters(&parameters);

			let tile_size = args.tile_size.unwrap_or(512);
			let cache = Arc::new(Mutex::new(CacheMap::new(factory.get_config())));
			let traversal = Traversal::new(TraversalOrder::DepthFirst, BLOCK_TILE_COUNT, BLOCK_TILE_COUNT)?;

			Ok(Box::new(Self {
				cache,
				parameters,
				source,
				tilejson,
				level_base,
				tile_size,
				traversal,
			}) as Box<dyn OperationTrait>)
		})
	}

	#[context("Failed to add images to cache from container {container:?}")]
	async fn add_images_to_cache(&self, container: &TileBBoxMap<Option<DynamicImage>>) -> Result<()> {
		log::trace!("add_images_to_cache: {:?}", container.bbox());

		let bbox = container.bbox();
		if bbox.level == 0 || bbox.level > self.level_base {
			return Ok(());
		};

		if bbox.width() > BLOCK_TILE_COUNT || bbox.height() > BLOCK_TILE_COUNT {
			bail!("Container bbox is too large: {:?}", bbox);
		}

		let full_size = self.tile_size;

		let images: Vec<(TileCoord, Option<DynamicImage>)> =
			futures::future::join_all(container.iter().map(|(coord, item)| {
				let item = item.clone();
				tokio::task::spawn_blocking(move || {
					if let Some(image) = item {
						assert_eq!(image.width(), full_size);
						assert_eq!(image.height(), full_size);
						(coord, Some(image.get_scaled_down(2).unwrap()))
					} else {
						(coord, None)
					}
				})
			}))
			.await
			.into_iter()
			.collect::<Result<Vec<_>, _>>()?;

		let mut coord = bbox.min_corner();
		coord.floor(BLOCK_TILE_COUNT);
		let mut cache = self.cache.lock().await;
		cache.insert(&coord, images)?;

		Ok(())
	}

	#[context("Failed to build images from cache for bbox {bbox:?}")]
	async fn build_images_from_cache(&self, bbox: TileBBox) -> Result<TileBBoxMap<Option<DynamicImage>>> {
		log::trace!("build_images_from_cache: {:?}", bbox);

		let size = bbox.max_count().min(BLOCK_TILE_COUNT);

		ensure!(bbox.level < self.level_base, "Invalid level");
		ensure!(bbox.width() <= size, "Invalid width");
		ensure!(bbox.height() <= size, "Invalid height");

		let bbox0 = bbox.rounded(size);
		assert_eq!(bbox0.width(), size);
		assert_eq!(bbox0.height(), size);

		let mut map: TileBBoxMap<Vec<(TileCoord, DynamicImage)>> = TileBBoxMap::new_default(bbox);

		let full_size = self.tile_size;
		let half_size = self.tile_size / 2;

		// get images from cache
		let mut cache = self.cache.lock().await;
		for q in &[0, 1, 2, 3] {
			let bbox1 = bbox0.leveled_up().get_quadrant(*q)?;

			if let Some(images1) = cache.remove(&bbox1.min_corner())? {
				for (coord1, image1) in images1 {
					if let Some(image1) = image1 {
						assert_eq!(image1.width(), half_size);
						assert_eq!(image1.height(), half_size);
						let coord0 = coord1.as_level_decreased()?;
						if bbox.contains(&coord0) {
							map.get_mut(&coord0)?.push((coord1, image1));
						}
					}
				}
			}
		}
		drop(cache);

		let vec: Vec<(TileCoord, DynamicImage)> = map
			.into_iter()
			.flat_map(|(coord0, sub_entries)| {
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
}

#[async_trait]
impl OperationTrait for Operation {
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		&self.traversal
	}

	async fn get_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox);

		if bbox.level > self.level_base {
			return self.source.get_stream(bbox).await;
		}

		let size = bbox.max_count().min(BLOCK_TILE_COUNT);
		let mut bbox0 = bbox.rounded(size);
		assert_eq!(bbox0.width(), size);
		assert_eq!(bbox0.height(), size);
		bbox0.intersect_with_pyramid(&self.parameters.bbox_pyramid);

		let container: TileBBoxMap<Option<DynamicImage>> = if bbox.level == self.level_base {
			log::trace!("Fetching images from source for bbox {:?}", bbox);
			TileBBoxMap::<Option<DynamicImage>>::from_stream(
				bbox,
				self
					.source
					.get_stream(bbox)
					.await?
					.map_item_parallel(|tile| Ok(tile.into_image())),
			)
			.await?
		} else {
			log::trace!("Building images from cache for bbox {:?}", bbox);
			self.build_images_from_cache(bbox0).await?
		};

		log::trace!("Adding images to cache for bbox {:?}", container.bbox());
		self.add_images_to_cache(&container).await?;

		let format = self.source.parameters().tile_format;

		log::trace!("Composing final stream for bbox {:?}", bbox);
		let vec = container
			.into_iter()
			.filter_map(move |(c, o)| {
				if let Some(image) = o {
					if bbox.contains(&c) {
						Some((c, Tile::from_image(image, format)))
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

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"raster_overview"
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, source, factory).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use imageproc::image::GenericImageView;
	use versatiles_core::config::Config;

	fn make_operation(tile_size: u32, level_base: u8) -> Operation {
		let parameters = TilesReaderParameters::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			TileBBoxPyramid::new_full(level_base),
		);

		let pyramid = TileBBoxPyramid::from_geo_bbox(
			level_base,
			level_base,
			&GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap(),
		);
		Operation {
			parameters,
			source: Box::new(DummyImageSource::new("F00.png", Some(pyramid), tile_size).unwrap()),
			tilejson: TileJSON::default(),
			level_base,
			tile_size,
			traversal: Traversal::new_any_size(1, 1).unwrap(),
			cache: Arc::new(Mutex::new(CacheMap::new(Config::default().arc()))),
		}
	}

	fn solid_rgba(size: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
		let color = imageproc::image::Rgba([r, g, b, a]);
		let mut img = DynamicImage::new_rgba8(size, size);
		for y in 0..size {
			for x in 0..size {
				img.put_pixel(x, y, color);
			}
		}
		img
	}

	#[tokio::test]
	async fn add_images_to_cache_inserts_half_tiles_under_floored_key() -> Result<()> {
		let op = make_operation(2, 6); // tiny tiles to keep work light
		let bbox = TileBBox::from_min_and_max(6, 0, 0, 31, 31)?; // 32x32 block at base level
		let mut container = TileBBoxMap::new_default(bbox);
		// Populate with simple solid tiles (only a tiny subset to keep it cheap)
		for y in 0..bbox.height() {
			for x in 0..bbox.width() {
				let c = TileCoord::new(6, bbox.x_min() + x, bbox.y_min() + y)?;
				container.insert(c, Some(solid_rgba(2, x as u8, y as u8, 32, 255)))?;
			}
		}

		op.add_images_to_cache(&container).await?;

		// Cache key should be the floored corner of the container bbox at level 6
		let mut cache = op.cache.lock().await;
		let key = TileCoord::new(6, 0, 0)?;
		assert!(cache.contains_key(&key));

		let stored = cache.remove(&key)?.expect("value stored");
		// Stored entries are (coord, Option<img>) with half-size images (1x1 at tile_size=2)
		assert!(!stored.is_empty());

		for (coord, img_opt) in &stored {
			assert_eq!(coord.level, 6);
			assert!(img_opt.is_some());
			assert_eq!(img_opt.as_ref().unwrap().width(), 1);
			assert_eq!(img_opt.as_ref().unwrap().height(), 1);
		}

		Ok(())
	}

	#[tokio::test]
	async fn build_images_from_cache_composes_quadrants() -> Result<()> {
		let op = make_operation(2, 6);

		// Prepare cache content by adding a full 32x32 block at level 6
		let bbox_lvl6 = TileBBox::from_min_and_size(6, 0, 0, 32, 32)?;
		let mut cont6 = TileBBoxMap::new_default(bbox_lvl6);
		for y in 0..bbox_lvl6.height() {
			for x in 0..bbox_lvl6.width() {
				let c = TileCoord::new(6, x, y)?;
				cont6.insert(c, Some(solid_rgba(2, x as u8, y as u8, 0, 255)))?;
			}
		}
		op.add_images_to_cache(&cont6).await?;

		// Now request composed images at level 5 for a tiny bbox (2x2 tiles)
		let bbox_lvl5 = TileBBox::from_min_and_size(5, 0, 0, 2, 2)?;
		let result = op.build_images_from_cache(bbox_lvl5).await?;
		let items: Vec<_> = result.into_iter().collect();
		// We expect at least one composed tile present (others may be missing if cache quadrants absent)
		assert!(!items.is_empty());

		for (coord, img_opt) in &items {
			assert_eq!(coord.level, 5);
			assert!(img_opt.is_some());
			let img = img_opt.as_ref().unwrap();
			assert_eq!(img.width(), 2);
			assert_eq!(img.height(), 2);
			// Check pixel colors to verify correct quadrant composition
			let r0 = coord.x as u8 * 2;
			let g0 = coord.y as u8 * 2;
			assert_eq!(img.get_pixel(0, 0).0, [r0, g0, 0, 255]);
			assert_eq!(img.get_pixel(0, 1).0, [r0, g0 + 1, 0, 255]);
			assert_eq!(img.get_pixel(1, 0).0, [r0 + 1, g0, 0, 255]);
			assert_eq!(img.get_pixel(1, 1).0, [r0 + 1, g0 + 1, 0, 255]);
		}

		Ok(())
	}
}
