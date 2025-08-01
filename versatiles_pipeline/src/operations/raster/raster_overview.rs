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
	cache: Arc<Mutex<HashMap<TileCoord3, Option<DynamicImage>>>>,
}

static MAX_BLOCK_TILE_SIZE: u32 = 64;

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
				traversal: Traversal::new(TraversalOrder::DepthFirst, MAX_BLOCK_TILE_SIZE, MAX_BLOCK_TILE_SIZE)?,
			}) as Box<dyn OperationTrait>)
		})
	}
	async fn add_image_to_cache(&self, coord: &TileCoord3, optional_image: &Option<DynamicImage>) {
		if let Some(image) = optional_image {
			let image = image.get_scaled_down(2);
			let mut cache = self.cache.lock().await;
			cache.insert(*coord, Some(image));
		} else {
			let mut cache = self.cache.lock().await;
			cache.insert(*coord, None);
		}
	}
	async fn add_stream_to_cache(&self, images: &[(TileCoord3, DynamicImage)]) {
		let mut cache = self.cache.lock().await;
		for (coord, image) in images {
			let image = image.get_scaled_down(2);
			cache.insert(*coord, Some(image));
		}
	}
	async fn get_image_from_cache(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		let cache = self.cache.lock().await;
		if let Some(image) = cache.get(coord) {
			return Ok(image.clone());
		}
		bail!("Not found")
	}
	async fn get_stream_from_cache(&self, bbox: &TileBBox) -> Vec<(TileCoord3, DynamicImage)> {
		let mut cache = self.cache.lock().await;
		let mut result = Vec::new();
		for coord in bbox.iter_coords() {
			if let Some(Some(image)) = cache.remove(&coord) {
				result.push((coord, image));
			}
		}
		result
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
		if coord.z > self.base_level {
			return self.source.get_image_data(coord).await;
		}

		if coord.z == self.base_level {
			let image = self.source.get_image_data(coord).await.unwrap();
			self.add_image_to_cache(coord, &image).await;
			return Ok(image);
		}

		let mut tile = DynamicImage::new_rgba8(self.tile_size, self.tile_size);
		let bbox = coord.as_tile_bbox(1)?.get_next_level();
		for coord2 in bbox.into_iter_coords() {
			let optional_sub_image = if let Ok(sub_image) = self.get_image_from_cache(&coord2).await {
				sub_image
			} else {
				self
					.source
					.get_image_data(&coord2)
					.await?
					.map(|image| image.get_scaled_down(2))
			};
			if let Some(sub_image) = optional_sub_image {
				tile.copy_from(
					&sub_image,
					(coord2.x - coord.x * 2) * self.tile_size,
					(coord2.y - coord.y * 2) * self.tile_size,
				)?;
			}
		}
		let optional_tile = Some(tile);
		self.add_image_to_cache(coord, &optional_tile).await;
		return Ok(optional_tile);
	}

	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		if bbox.level > self.base_level {
			return self.source.get_image_stream(bbox).await;
		}

		if bbox.level == self.base_level {
			let images = self.source.get_image_stream(bbox).await.unwrap().collect().await;
			self.add_stream_to_cache(images.as_slice()).await;
			return Ok(TileStream::from_vec(images));
		}

		let w = bbox.width();
		let h = bbox.height();
		assert!(w <= MAX_BLOCK_TILE_SIZE && h <= MAX_BLOCK_TILE_SIZE);

		let mut super_tile = DynamicImage::new_rgba8(self.tile_size * w, self.tile_size * h);
		let images = self.get_stream_from_cache(&bbox.get_next_level()).await;
		for (coord2, sub_image) in images {
			super_tile.copy_from(
				&sub_image,
				(coord2.x - bbox.x_min * 2) * self.tile_size / 2,
				(coord2.y - bbox.y_min * 2) * self.tile_size / 2,
			)?;
		}

		let mut result = Vec::new();
		for coord in bbox.into_iter_coords() {
			let tile = super_tile.crop(
				(coord.x - bbox.x_min) * self.tile_size,
				(coord.y - bbox.y_min) * self.tile_size,
				self.tile_size,
				self.tile_size,
			);
			result.push((coord, tile));
		}

		self.add_stream_to_cache(result.as_slice()).await;

		return Ok(TileStream::from_vec(result));
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		if coord.z >= self.base_level {
			return self.source.get_tile_data(coord).await;
		} else {
			return pack_image_tile(self.get_image_data(coord).await, &self.parameters);
		}
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		if bbox.level >= self.base_level {
			return self.source.get_tile_stream(bbox).await;
		} else {
			return pack_image_tile_stream(self.get_image_stream(bbox).await, &self.parameters);
		}
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
