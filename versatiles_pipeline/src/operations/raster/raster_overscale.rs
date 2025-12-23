use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use lru::LruCache;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles_container::Tile;
use versatiles_core::*;
use versatiles_derive::context;
use versatiles_image::{DynamicImage, traits::*};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// use this zoom level to build the overscale. Defaults to the maximum zoom level of the source.
	level_base: Option<u8>,
	/// use this as maximum zoom level. Defaults to 30.
	level_max: Option<u8>,
	/// Size of the tiles in pixels. Defaults to 512.
	tile_size: Option<u32>,
}

#[derive(Debug)]
struct TileCache {
	lru: LruCache<TileCoord, Arc<DynamicImage>>,
	max_memory_bytes: usize,
	current_memory_bytes: usize,
}

impl TileCache {
	fn new(max_memory_mb: usize) -> Self {
		Self {
			lru: LruCache::unbounded(),
			max_memory_bytes: max_memory_mb * 1024 * 1024,
			current_memory_bytes: 0,
		}
	}

	fn insert(&mut self, coord: TileCoord, image: Arc<DynamicImage>) {
		let image_bytes = (image.width() * image.height() * 4) as usize;

		// Evict until we have space
		while self.current_memory_bytes + image_bytes > self.max_memory_bytes {
			if let Some((_, evicted)) = self.lru.pop_lru() {
				let evicted_bytes = (evicted.width() * evicted.height() * 4) as usize;
				self.current_memory_bytes -= evicted_bytes;
			} else {
				break;
			}
		}

		self.lru.put(coord, image);
		self.current_memory_bytes += image_bytes;
	}

	fn get(&mut self, coord: &TileCoord) -> Option<Arc<DynamicImage>> {
		self.lru.get(coord).cloned()
	}
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	source: Arc<Box<dyn OperationTrait>>,
	tilejson: TileJSON,
	level_base: u8,
	level_min: u8,
	tile_size: u32,
	cache: Arc<Mutex<TileCache>>,
}

impl Clone for Operation {
	fn clone(&self) -> Self {
		Self {
			parameters: self.parameters.clone(),
			source: Arc::clone(&self.source),
			tilejson: self.tilejson.clone(),
			level_base: self.level_base,
			level_min: self.level_min,
			tile_size: self.tile_size,
			cache: Arc::clone(&self.cache),
		}
	}
}

impl Operation {
	#[context("Building raster_overscale operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn OperationTrait>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + OperationTrait,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let mut parameters = source.as_ref().parameters().clone();

		let level_base = args
			.level_base
			.unwrap_or(source.as_ref().parameters().bbox_pyramid.get_level_max().unwrap());
		log::trace!("level_base {}", level_base);

		let level_max = args.level_max.unwrap_or(30).clamp(level_base, 30);

		let mut level_bbox = *parameters.bbox_pyramid.get_level_bbox(level_base);
		while level_bbox.level <= level_max {
			level_bbox.level_up();
			parameters.bbox_pyramid.set_level_bbox(level_bbox);
		}

		let mut tilejson = source.as_ref().tilejson().clone();
		tilejson.update_from_reader_parameters(&parameters);

		let level_min = source.as_ref().parameters().bbox_pyramid.get_level_min().unwrap_or(0);
		let cache = Arc::new(Mutex::new(TileCache::new(512)));

		Ok(Self {
			parameters,
			source: Arc::new(source),
			tilejson,
			level_base,
			level_min,
			tile_size: args.tile_size.unwrap_or(512),
			cache,
		})
	}

	async fn get_tile_with_climbing(&self, coord_dst: TileCoord) -> Result<Option<Tile>> {
		let level_dst = coord_dst.level;
		let mut search_level = self.level_base.min(level_dst);
		let mut coord_src = coord_dst.at_level(search_level);

		loop {
			// 1. Check cache
			{
				let mut cache = self.cache.lock().await;
				if let Some(cached_image) = cache.get(&coord_src) {
					drop(cache);
					return Ok(Some(Tile::from_image(
						extract_image(&cached_image, coord_src, coord_dst)?,
						self.parameters.tile_format,
					)?));
				}
			}

			// 2. Try to fetch from source
			let bbox = coord_src.to_tile_bbox();
			let mut stream = self.source.as_ref().get_stream(bbox).await?;

			if let Some((found_coord, tile)) = stream.next().await
				&& found_coord == coord_src
			{
				let image = tile.into_image()?;
				let image_arc = Arc::new(image);

				// Cache it
				{
					let mut cache = self.cache.lock().await;
					cache.insert(coord_src, image_arc.clone());
				}

				return Ok(Some(Tile::from_image(
					extract_image(&image_arc, coord_src, coord_dst)?,
					self.parameters.tile_format,
				)?));
			}

			// 3. Tile not found - climb to parent
			if search_level <= self.level_min {
				return Ok(None);
			}

			search_level -= 1;
			coord_src = coord_src.as_level_decreased()?;
		}
	}
}

fn extract_image(image_src: &DynamicImage, coord_src: TileCoord, coord_dst: TileCoord) -> Result<DynamicImage> {
	let level_diff = coord_dst.level as i32 - coord_src.level as i32;

	ensure!(level_diff >= 0, "difference in levels must be non-negative");

	if level_diff == 0 {
		return Ok((*image_src).clone());
	}

	// Calculate extraction parameters
	let scale = 1 << level_diff;
	let tile_size = image_src.width(); // Assume square tiles
	let sub_size = tile_size as f64 / scale as f64;
	let tile_offset_x = (coord_dst.x % scale) as f64;
	let tile_offset_y = (coord_dst.y % scale) as f64;
	let x0 = tile_offset_x * sub_size;
	let y0 = tile_offset_y * sub_size;

	image_src.get_extract(x0, y0, sub_size, sub_size, tile_size, tile_size)
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
		self.source.as_ref().traversal()
	}

	#[context("Failed to get stream for bbox: {:?}", bbox_dst)]
	async fn get_stream(&self, bbox_dst: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox_dst);

		if !self.parameters.bbox_pyramid.overlaps_bbox(&bbox_dst) {
			log::trace!("get_stream outside bbox_pyramid");
			return Ok(TileStream::empty());
		}

		if bbox_dst.level <= self.level_base {
			log::trace!("get_stream level <= level_base");
			return self.source.as_ref().get_stream(bbox_dst).await;
		}

		// Use tile climbing for all upscaling - process in parallel
		let coords: Vec<TileCoord> = bbox_dst.into_iter_coords().collect();
		let self_arc = Arc::new(self.clone()); // Share Operation across tasks

		let stream = TileStream::from_coord_vec_async(coords, move |coord| {
			let self_arc = Arc::clone(&self_arc);
			async move {
				match self_arc.get_tile_with_climbing(coord).await {
					Ok(Some(tile)) => Some((coord, tile)),
					Ok(None) => None,
					Err(e) => {
						log::warn!("Failed to get tile {:?}: {}", coord, e);
						None
					}
				}
			}
		});

		Ok(stream)
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"raster_overscale"
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
		Operation::build(vpl_node, source, factory)
			.await
			.map(|op| Box::new(op) as Box<dyn OperationTrait>)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use rstest::rstest;
	use versatiles_image::DynamicImage;

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

	async fn get_avg(op: &Operation, coord: (u8, u8, u8), scale: u32) -> Vec<u8> {
		let (level, x, y) = coord;
		let coord = TileCoord::new(level, x as u32, y as u32).unwrap().to_tile_bbox();
		let mut tiles = op.get_stream(coord).await.unwrap().to_vec().await;
		assert_eq!(tiles.len(), 1);
		let mut tile = tiles.pop().unwrap().1;
		let image = tile.as_image().unwrap();
		let avg = image.average_color();
		avg.into_iter()
			.map(|c| (c as f64 / scale as f64).round() as u8)
			.collect()
	}

	async fn build_op(channel_count: usize) -> Result<Operation> {
		let image = make_gradient_image(channel_count);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		Operation::build(
			VPLNode::try_from_str("raster_overscale tile_size=256 level_base=2")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await
	}

	#[rstest]
	#[case::l(1,[vec![16], vec![48], vec![16], vec![48]])]
	#[case::la(2,[vec![16,16], vec![48,16], vec![16,48], vec![48,48]])]
	#[case::rgb(3,[vec![16,16,48], vec![48,16,16], vec![16,48,48], vec![48,48,16]])]
	#[case::rgba(4,[vec![16,16,48,48], vec![48,16,16,48], vec![16,48,48,16], vec![48,48,16,16]])]
	#[tokio::test]
	async fn overscale_to_z3(#[case] channel_count: usize, #[case] expected: [Vec<u8>; 4]) {
		let op = build_op(channel_count).await.unwrap();

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
		let op = build_op(3).await?;

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
}
