use crate::{
	PipelineFactory,
	helpers::{pack_image_tile, pack_image_tile_stream},
	traits::*,
	vpl::VPLNode,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::{DynamicImage, GenericImage};
use std::{collections::HashMap, fmt::Debug, sync::Arc};
use tokio::sync::Mutex;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::EnhancedDynamicImageTrait;

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
	base_level: u8,
	tile_size: u32,
	traversal: Traversal,
	cache: Arc<Mutex<HashMap<TileCoord3, Vec<(TileCoord3, DynamicImage)>>>>,
}

static BLOCK_TILE_COUNT: u32 = 32;

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
			let mut parameters = source.parameters().clone();

			let base_level = args
				.level
				.unwrap_or_else(|| source.parameters().bbox_pyramid.get_zoom_max().unwrap());

			let mut level_bbox = *parameters.bbox_pyramid.get_level_bbox(base_level);
			while level_bbox.level > 0 {
				level_bbox.scale_down(2);
				level_bbox.level -= 1;
				parameters.bbox_pyramid.set_level_bbox(level_bbox);
			}

			let mut tilejson = source.tilejson().clone();
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				cache: Arc::new(Mutex::new(HashMap::new())),
				parameters,
				source,
				tilejson,
				base_level,
				tile_size: args.tile_size.unwrap_or(512),
				traversal: Traversal::new(TraversalOrder::DepthFirst, BLOCK_TILE_COUNT, BLOCK_TILE_COUNT)?,
			}) as Box<dyn OperationTrait>)
		})
	}

	async fn add_images_to_cache(&self, key: TileCoord3, images: &[(TileCoord3, DynamicImage)]) {
		let images = images
			.iter()
			.map(|(coord, image)| (*coord, image.get_scaled_down(2)))
			.filter(|(_, image)| !image.is_empty())
			.collect::<Vec<_>>();
		let mut cache = self.cache.lock().await;
		cache.insert(key, images);
	}

	#[context("Failed to get images from cache for key {key:?}")]
	async fn get_images_from_cache(&self, key: &TileCoord3) -> Result<Vec<(TileCoord3, DynamicImage)>> {
		let mut cache = self.cache.lock().await;
		let result = cache.remove(key);
		if let Some(images) = result {
			return Ok(images);
		} else {
			let max_value = 2u32.pow(key.level as u32) - 1;

			let bbox = TileBBox::new(
				key.level,
				(key.x * BLOCK_TILE_COUNT).min(max_value),
				(key.y * BLOCK_TILE_COUNT).min(max_value),
				(key.x * BLOCK_TILE_COUNT + BLOCK_TILE_COUNT - 1).min(max_value),
				(key.y * BLOCK_TILE_COUNT + BLOCK_TILE_COUNT - 1).min(max_value),
			)
			.unwrap();
			return self.build_images_from_source(&bbox, self.tile_size / 2).await;
		}
	}

	#[context("Failed to build images from source for bbox {bbox_res:?} and image size {image_size_res}")]
	async fn build_images_from_source(
		&self,
		bbox_res: &TileBBox,
		image_size_res: u32,
	) -> Result<Vec<(TileCoord3, DynamicImage)>> {
		let level_res = bbox_res.level;
		let level_src = self.base_level;
		assert!(level_res <= level_src);

		assert!(image_size_res > 0);
		assert!(image_size_res.is_power_of_two());

		let bbox_src = bbox_res.as_level(level_src);

		let map_res = Arc::new(Mutex::new(HashMap::<TileCoord3, DynamicImage>::new()));

		let stream = self.source.get_image_stream(bbox_src).await?;

		let scale_factor = 2u32.pow(level_src as u32 - level_res as u32);
		assert!(scale_factor > 0);

		let (image_size_tmp, image_size_src) = if scale_factor < image_size_res {
			(image_size_res, image_size_res / scale_factor)
		} else {
			(scale_factor, 1)
		};

		let scale_src_tmp = self.tile_size / image_size_src;
		assert!(scale_src_tmp > 0);
		assert!(scale_src_tmp <= self.tile_size);

		let scale_tmp_res = image_size_tmp / image_size_res;
		assert!(scale_tmp_res > 0);
		assert!(scale_tmp_res <= self.tile_size);

		stream
			.for_each_async(|(coord_src, image_src)| {
				assert_eq!(image_src.width(), self.tile_size);
				assert_eq!(image_src.height(), self.tile_size);

				let map = map_res.clone();
				async move {
					let coord_res = coord_src.as_level(level_res);
					let image_src = image_src.into_scaled_down(scale_src_tmp);
					let mut db = map.lock().await;
					let image0 = db
						.entry(coord_res)
						.or_insert_with(|| DynamicImage::new_rgba8(image_size_tmp, image_size_tmp));
					image0
						.copy_from(
							&image_src,
							coord_src.x * image_size_src - coord_res.x * image_size_tmp,
							coord_src.y * image_size_src - coord_res.y * image_size_tmp,
						)
						.unwrap();
				}
			})
			.await;

		let db = Arc::try_unwrap(map_res)
			.map_err(|_| anyhow::anyhow!("Failed to unwrap Arc"))?
			.into_inner();
		Ok(db
			.into_iter()
			.map(|(coord, image_tmp)| {
				let image_res = image_tmp.into_scaled_down(scale_tmp_res);
				(coord, image_res)
			})
			.filter(|(_, image)| !image.is_empty())
			.collect())
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

	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		if coord.level >= self.base_level {
			return self.source.get_image_data(coord).await;
		}

		let stream = self
			.build_images_from_source(&coord.as_tile_bbox(1)?, self.tile_size)
			.await?;
		let mut tiles = stream.to_vec();
		Ok(tiles.pop().map(|(_, image)| image))
	}

	async fn get_image_stream(&self, bbox_0: TileBBox) -> Result<TileStream<DynamicImage>> {
		if bbox_0.level >= self.base_level {
			return self.source.get_image_stream(bbox_0).await;
		}

		assert!(bbox_0.width() <= BLOCK_TILE_COUNT);
		assert!(bbox_0.height() <= BLOCK_TILE_COUNT);

		let key = bbox_0.get_scaled_down(BLOCK_TILE_COUNT);
		assert_eq!(key.get_dimensions(), (1, 1));

		let mut result = HashMap::<TileCoord3, DynamicImage>::new();
		for (x1, y1) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
			let mut key1 = bbox_0
				.get_scaled_down(BLOCK_TILE_COUNT)
				.as_level_increased()
				.get_corner_min();
			key1.x += x1 as u32;
			key1.y += y1 as u32;
			let images1 = self.get_images_from_cache(&key1).await?;
			for (coord1, image1) in images1 {
				let coord0 = coord1.as_level(bbox_0.level);
				result
					.entry(coord0)
					.or_insert_with(|| DynamicImage::new_rgba8(self.tile_size, self.tile_size))
					.copy_from(
						&image1,
						(coord1.x - coord0.x * 2) * self.tile_size / 2,
						(coord1.y - coord0.y * 2) * self.tile_size / 2,
					)?;
			}
		}
		let images0 = result.into_iter().collect::<Vec<_>>();

		self.add_images_to_cache(key.get_corner_min(), images0.as_slice()).await;

		return Ok(TileStream::from_vec(images0));
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		if coord.level >= self.base_level {
			return self.source.get_tile_data(coord).await;
		} else {
			return pack_image_tile(self.get_image_data(coord).await, &self.parameters);
		}
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		if bbox.level >= self.base_level {
			return self.source.get_tile_stream(bbox).await;
		}
		pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters)
	}

	async fn get_vector_data(&self, _coord: &TileCoord3) -> Result<Option<VectorTile>> {
		bail!("Vector tiles are not supported in raster_overview operations.");
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
