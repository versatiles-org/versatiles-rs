use crate::{
	PipelineFactory,
	helpers::{pack_image_tile, pack_image_tile_stream},
	traits::*,
	vpl::VPLNode,
};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::{DynamicImage, GenericImage};
use std::{collections::HashMap, fmt::Debug, sync::Arc};
use tokio::{sync::Mutex, task::JoinSet};
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::EnhancedDynamicImageTrait;

static BLOCK_TILE_COUNT: u32 = 32;
type Tiles = Vec<(TileCoord3, DynamicImage)>;

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
	cache: Arc<Mutex<HashMap<TileCoord3, Tiles>>>,
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

	#[context("Failed to build overviews from {} source tiles by reducing level by {level_reduce}", tiles.len())]
	async fn build_overviews(
		&self,
		level_reduce: u8,
		tiles: &[(TileCoord3, DynamicImage)],
	) -> Result<Vec<(TileCoord3, DynamicImage)>> {
		let scale_factor = 2u32.pow(level_reduce as u32);

		let (image_size_tmp, image_size_src) = if scale_factor <= self.tile_size {
			(self.tile_size, self.tile_size / scale_factor)
		} else {
			(scale_factor, 1)
		};

		let scale_src_tmp = self.tile_size / image_size_src;
		assert!(scale_src_tmp > 0);
		assert!(scale_src_tmp <= self.tile_size);

		let scale_tmp_res = image_size_tmp / self.tile_size;
		assert!(scale_tmp_res > 0);
		assert!(scale_tmp_res <= self.tile_size);

		// Sort tiles by their destination coordinates
		let mut map = HashMap::<TileCoord3, Vec<(&TileCoord3, &DynamicImage)>>::new();
		for (coord_src, image_src) in tiles {
			ensure!(image_src.width() == self.tile_size);
			ensure!(image_src.height() == self.tile_size);

			ensure!(coord_src.level >= level_reduce);
			let coord_dst = coord_src.as_level(coord_src.level - level_reduce);

			map.entry(coord_dst).or_default().push((coord_src, image_src));
		}

		let map = unsafe {
			std::mem::transmute::<
				HashMap<TileCoord3, Vec<(&TileCoord3, &DynamicImage)>>,
				HashMap<TileCoord3, Vec<(&'static TileCoord3, &'static DynamicImage)>>,
			>(map)
		};

		let results = map
			.into_iter()
			.map(|(coord_dst, sub_entries)| async move {
				ensure!(sub_entries.len() <= (scale_factor * scale_factor) as usize);
				let mut image_tmp = DynamicImage::new_rgba8(image_size_tmp, image_size_tmp);
				for (coord_src, image_src) in sub_entries {
					let image_src = image_src.get_scaled_down(scale_src_tmp);
					image_tmp.copy_from(
						&image_src,
						(coord_src.x % scale_factor) * image_size_src,
						(coord_src.y % scale_factor) * image_size_src,
					)?;
				}
				let image_res = image_tmp.into_scaled_down(scale_tmp_res);
				Ok((coord_dst, image_res.into_optional()))
			})
			.collect::<JoinSet<_>>()
			.join_all()
			.await;

		let results = results
			.into_iter()
			.collect::<Result<Vec<_>>>()
			.map_err(|e| e.context("Failed to build overviews from source tiles"))?;

		let results = results
			.into_iter()
			.filter_map(|(coord, image_option)| image_option.map(|image| (coord, image)))
			.collect::<Vec<_>>();

		Ok(results)
	}

	#[context("Failed to build images from source for bbox {bbox_dst:?}")]
	async fn build_images_from_source(&self, bbox_dst: &TileBBox) -> Result<Vec<(TileCoord3, DynamicImage)>> {
		ensure!(bbox_dst.x_min / BLOCK_TILE_COUNT == bbox_dst.x_max / BLOCK_TILE_COUNT);
		ensure!(bbox_dst.y_min / BLOCK_TILE_COUNT == bbox_dst.y_max / BLOCK_TILE_COUNT);

		let level_src = self.base_level;
		let level_dst = bbox_dst.level;
		assert!(level_dst <= level_src);

		let bbox_src = bbox_dst.as_level(level_src);
		ensure!(bbox_src.count_tiles() <= BLOCK_TILE_COUNT as u64 * BLOCK_TILE_COUNT as u64);

		let tiles1 = self.source.get_image_stream(bbox_src).await?.to_vec().await;
		let tiles0 = self.build_overviews(level_src - level_dst, &tiles1).await?;

		Ok(tiles0)
	}

	#[context("Failed to convert TileBBox {bbox:?} to cache key")]
	fn bbox_to_cache_key(bbox: &TileBBox) -> Result<TileCoord3> {
		let scale = BLOCK_TILE_COUNT / 2;
		ensure!(
			bbox.x_min / scale == bbox.x_max / scale,
			"TileBBox is wider than {scale}",
		);
		ensure!(
			bbox.y_min / scale == bbox.y_max / scale,
			"TileBBox is taller than {scale}",
		);

		TileCoord3::new(bbox.level, bbox.x_min / scale, bbox.y_min / scale)
	}

	#[context("Failed to convert cache key {key:?} to TileBBox")]
	fn cache_key_to_bbox(key: &TileCoord3) -> Result<TileBBox> {
		let max = 2u32.pow(key.level as u32) - 1;
		let scale = BLOCK_TILE_COUNT / 2;
		TileBBox::new(
			key.level,
			(key.x * scale).min(max),
			(key.y * scale).min(max),
			((key.x + 1) * scale - 1).min(max),
			((key.y + 1) * scale - 1).min(max),
		)
	}

	#[context("Failed to get images from cache for key {key:?}")]
	async fn get_images_from_cache(&self, key: &TileCoord3) -> Result<Tiles> {
		let mut cache = self.cache.lock().await;
		let result = cache.remove(key);
		if let Some(images) = result {
			return Ok(images);
		} else {
			let bbox = Self::cache_key_to_bbox(key)?;
			return self.build_images_from_source(&bbox).await;
		}
	}

	#[context("Failed to add {} images to cache for bbox {bbox:?}", images.len())]
	async fn add_images_to_cache(&self, bbox: &TileBBox, images: &[(TileCoord3, DynamicImage)]) -> Result<()> {
		if bbox.level > 0 {
			let tiles = self.build_overviews(1, images).await?;
			let key0 = Self::bbox_to_cache_key(&bbox.as_level_decreased())?;
			let mut cache = self.cache.lock().await;
			cache.insert(key0, tiles);
			if cache.len() > 30 {
				todo!("Cache size limit reached. Implement cache eviction strategy.");
			}
		}
		Ok(())
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

		let mut tiles = self.build_images_from_source(&coord.as_tile_bbox(1)?).await?;
		Ok(tiles.pop().map(|(_, image)| image))
	}

	async fn get_image_stream(&self, bbox0: TileBBox) -> Result<TileStream<DynamicImage>> {
		if bbox0.level >= self.base_level {
			return self.source.get_image_stream(bbox0).await;
		}

		assert!(bbox0.width() <= BLOCK_TILE_COUNT);
		assert!(bbox0.height() <= BLOCK_TILE_COUNT);

		let mut images0: Vec<(TileCoord3, DynamicImage)> = vec![];

		let tasks = bbox0
			.iter_bbox_grid(BLOCK_TILE_COUNT / 2)
			.map(move |bbox1| async move {
				let key1 = Self::bbox_to_cache_key(&bbox1)?;
				let images1 = self.get_images_from_cache(&key1).await?;
				Ok::<_, anyhow::Error>(images1)
			})
			.collect::<Vec<_>>();

		let results = futures::future::join_all(tasks).await;

		for result in results {
			let images1 = result?;
			images0.extend(images1);
		}

		self.add_images_to_cache(&bbox0, &images0).await?;

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
