use crate::{PipelineFactory, helpers::pack_image_tile_stream, traits::*, vpl::VPLNode};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::{DynamicImage, GenericImage};
use std::{collections::HashMap, fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
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
	cache: Arc<Mutex<HashMap<TileCoord3, Option<DynamicImage>>>>,
	#[allow(dead_code)]
	max_cache_size: usize,
}

impl Operation {
	fn build(
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		_factory: &PipelineFactory,
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
				.unwrap_or_else(|| source.parameters().bbox_pyramid.get_zoom_max().unwrap());

			let mut level_bbox = *parameters.bbox_pyramid.get_level_bbox(level_base);
			while level_bbox.level > 0 {
				level_bbox.level_decrease();
				parameters.bbox_pyramid.set_level_bbox(level_bbox);
			}

			let mut tilejson = source.tilejson().clone();
			tilejson.update_from_reader_parameters(&parameters);

			let tile_size = args.tile_size.unwrap_or(512);
			let max_cache_size = (1usize << 30) / ((tile_size as usize / 2).pow(2) * 4);
			let cache = Arc::new(Mutex::new(HashMap::new()));
			let traversal = Traversal::new(TraversalOrder::DepthFirst, BLOCK_TILE_COUNT, BLOCK_TILE_COUNT)?;

			Ok(Box::new(Self {
				cache,
				parameters,
				source,
				tilejson,
				level_base,
				max_cache_size,
				tile_size,
				traversal,
			}) as Box<dyn OperationTrait>)
		})
	}

	#[context("Failed to build half image from source for coord {coord_dst:?}")]
	async fn build_half_image_from_source(&self, coord_dst: &TileCoord3) -> Result<Option<DynamicImage>> {
		let half_size = self.tile_size / 2;

		let level_src = self.level_base;
		let level_dst = coord_dst.level;
		ensure!(level_dst <= level_src, "Invalid level");

		let scale = 1 << (level_src - level_dst + 1);
		ensure!(scale >= 1, "Scale < 1");
		ensure!(scale < self.tile_size, "Scale >= tile size");

		let scaled_size = self.tile_size / scale;
		ensure!(scaled_size > 0, "Scaled size is zero");

		let count = 1 << (level_src - level_dst);

		let bbox_src = coord_dst.as_tile_bbox(1)?.as_level(level_src);
		let mut image_dst = DynamicImage::new_rgba8(half_size, half_size);

		let mut stream_src = self.source.get_image_stream(bbox_src).await?;
		while let Some((coord, image)) = stream_src.next().await {
			if coord.level != level_src {
				continue;
			}

			let image_src = image.get_scaled_down(scale);
			let x = (coord.x % count) * scaled_size;
			let y = (coord.y % count) * scaled_size;

			image_dst.copy_from(&image_src, x, y)?;
		}

		Ok(image_dst.into_optional())
	}

	#[context("Failed to add images to cache from container {container:?}")]
	async fn add_images_to_cache(&self, container: &TileBBoxContainer<Option<DynamicImage>>) -> Result<()> {
		let bbox = container.bbox();
		if bbox.level == 0 || bbox.level > self.level_base {
			return Ok(());
		};

		let full_size = self.tile_size;

		let images: Vec<(TileCoord3, Option<DynamicImage>)> =
			futures::future::join_all(container.iter().map(|(coord, item)| {
				let item = item.clone();
				tokio::task::spawn_blocking(move || {
					if let Some(image) = item {
						assert_eq!(image.width(), full_size);
						assert_eq!(image.height(), full_size);
						(coord, Some(image.get_scaled_down(2)))
					} else {
						(coord, None)
					}
				})
			}))
			.await
			.into_iter()
			.collect::<Result<Vec<_>, _>>()?;

		let mut cache = self.cache.lock().await;
		for (coord, item) in images {
			cache.insert(coord, item);
		}
		Ok(())
	}

	#[context("Failed to build images from cache for bbox {bbox0:?}")]
	async fn build_images_from_cache(&self, bbox0: TileBBox) -> Result<TileBBoxContainer<Option<DynamicImage>>> {
		ensure!(bbox0.level < self.level_base);
		ensure!(bbox0.width() <= BLOCK_TILE_COUNT);
		ensure!(bbox0.height() <= BLOCK_TILE_COUNT);

		let bbox1 = bbox0.as_level(bbox0.level + 1);

		let mut map: TileBBoxContainer<Vec<(TileCoord3, DynamicImage)>> = TileBBoxContainer::new_default(bbox0);
		let mut misses = vec![];

		let full_size = self.tile_size;
		let half_size = self.tile_size / 2;

		// get images from cache
		let mut cache = self.cache.lock().await;
		for coord1 in bbox1.iter_coords() {
			if let Some(entry) = cache.remove(&coord1) {
				if let Some(image1) = entry {
					assert_eq!(image1.width(), half_size);
					assert_eq!(image1.height(), half_size);

					let coord0 = coord1.as_level(bbox0.level);
					map.get_mut(&coord0)?.push((coord1, image1));
				}
			} else {
				misses.push(coord1);
			}
		}
		drop(cache);

		// get missing images from source
		for coord1 in misses {
			if let Some(image1) = self.build_half_image_from_source(&coord1).await? {
				assert_eq!(image1.width(), half_size);
				assert_eq!(image1.height(), half_size);
				let coord0 = coord1.as_level(bbox0.level);
				map.get_mut(&coord0)?.push((coord1, image1));
			}
		}

		TileBBoxContainer::<Option<DynamicImage>>::from_iter(
			bbox0,
			map.into_iter().flat_map(|(coord0, sub_entries)| {
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
			}),
		)
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

	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		if bbox.level > self.level_base {
			return self.source.get_image_stream(bbox).await;
		}

		let bbox0 = bbox.get_rounded(BLOCK_TILE_COUNT);
		assert_eq!(bbox0.width(), BLOCK_TILE_COUNT);
		assert_eq!(bbox0.height(), BLOCK_TILE_COUNT);

		let container: TileBBoxContainer<Option<DynamicImage>> = if bbox.level == self.level_base {
			TileBBoxContainer::<Option<DynamicImage>>::from_stream(bbox, self.source.get_image_stream(bbox).await?).await?
		} else {
			self.build_images_from_cache(bbox).await?
		};

		self.add_images_to_cache(&container).await?;

		let vec = container
			.into_iter()
			.filter_map(move |(c, o)| {
				if let Some(image) = o {
					if bbox.contains3(&c) { Some((c, image)) } else { None }
				} else {
					None
				}
			})
			.collect();

		Ok(TileStream::from_vec(vec))
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		if bbox.level > self.level_base {
			return self.source.get_tile_stream(bbox).await;
		}
		pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters)
	}

	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("Vector tiles are not supported in raster_overview operations.");
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
mod tests {}
