use crate::{
	PipelineFactory,
	traits::{OperationFactoryTrait, ReadOperationFactoryTrait, TransformOperationFactoryTrait},
	vpl::VPLNode,
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use moka::future::Cache;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::{DynamicImage, traits::DynamicImageTraitOperation};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Raster overscale operation - generates tiles beyond the source's native resolution.
struct Args {
	/// use this zoom level to build the overscale. Defaults to the maximum zoom level of the source.
	level_base: Option<u8>,
	/// use this as maximum zoom level. Defaults to 30.
	level_max: Option<u8>,
	/// Enable tile climbing when overscaling. Defaults to false.
	enable_climbing: Option<bool>,
}

#[derive(Clone)]
struct Operation {
	metadata: TileSourceMetadata,
	source: Arc<Box<dyn TileSource>>,
	tilejson: TileJSON,
	level_base: u8,
	level_min: u8,
	enable_climbing: bool,
	cache: Arc<Cache<TileCoord, Arc<DynamicImage>>>,
}

impl Debug for Operation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Operation")
			.field("metadata", &self.metadata)
			.field("source", &self.source)
			.field("tilejson", &self.tilejson)
			.field("level_base", &self.level_base)
			.field("level_min", &self.level_min)
			.field("enable_climbing", &self.enable_climbing)
			.field("cache", &"<moka::future::Cache>")
			.finish()
	}
}

impl Operation {
	#[context("Building raster_overscale operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let mut metadata = source.as_ref().metadata().clone();

		let level_base = args
			.level_base
			.unwrap_or(source.as_ref().metadata().bbox_pyramid.get_level_max().unwrap());
		log::trace!("level_base {}", level_base);

		let level_max = args.level_max.unwrap_or(30).clamp(level_base, 30);

		let mut level_bbox = *metadata.bbox_pyramid.get_level_bbox(level_base);
		while level_bbox.level <= level_max {
			level_bbox.level_up();
			metadata.bbox_pyramid.set_level_bbox(level_bbox);
		}

		let mut tilejson = source.as_ref().tilejson().clone();
		metadata.update_tilejson(&mut tilejson);

		let level_min = source.as_ref().metadata().bbox_pyramid.get_level_min().unwrap_or(0);
		let cache = Cache::builder()
			.max_capacity(512 * 1024 * 1024) // 512MB limit
			.weigher(|_k: &TileCoord, v: &Arc<DynamicImage>| -> u32 {
				v.width() * v.height() * 4 // RGBA bytes
			})
			.build();
		let cache = Arc::new(cache);

		Ok(Self {
			metadata,
			source: Arc::new(source),
			tilejson,
			level_base,
			level_min,
			cache,
			enable_climbing: args.enable_climbing.unwrap_or(false),
		})
	}

	#[context("finding tile for coord {:?}", coord_dst)]
	async fn find_tile(
		&self,
		coord_dst: TileCoord,
		with_climbing: bool,
	) -> Result<Option<(TileCoord, Arc<DynamicImage>)>> {
		let mut coord_src = coord_dst.at_level(self.level_base.min(coord_dst.level));

		if with_climbing {
			loop {
				if let Some(image) = self.try_fetch_tile(coord_src).await? {
					return Ok(Some((coord_src, image)));
				}

				// Climb to parent
				if coord_src.level <= self.level_min {
					return Ok(None);
				}
				coord_src = coord_src.to_level_decreased()?;
			}
		} else {
			// Single attempt - no climbing
			if let Some(image) = self.try_fetch_tile(coord_src).await? {
				return Ok(Some((coord_src, image)));
			}
			Ok(None)
		}
	}

	/// Attempts to fetch a tile at the given coordinate, checking cache first.
	/// Returns None if the tile doesn't exist at this coordinate.
	async fn try_fetch_tile(&self, coord: TileCoord) -> Result<Option<Arc<DynamicImage>>> {
		// Check cache - no lock needed!
		if let Some(cached_image) = self.cache.get(&coord).await {
			return Ok(Some(cached_image));
		}

		// Fetch from source
		let bbox = coord.to_tile_bbox();
		let mut stream = self.source.get_tile_stream(bbox).await?;

		if let Some((found_coord, tile)) = stream.next().await
			&& found_coord == coord
		{
			let image = Arc::new(tile.into_image()?);

			// Cache it - no lock needed!
			self.cache.insert(coord, image.clone()).await;

			Ok(Some(image))
		} else {
			Ok(None)
		}
	}
}

#[context("extracting image for tile {:?}", coord_dst)]
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
impl TileSource for Operation {
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
	async fn get_tile_stream(&self, bbox_dst: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox_dst);

		if !self.metadata.bbox_pyramid.overlaps_bbox(&bbox_dst) {
			log::trace!("get_stream outside bbox_pyramid");
			return Ok(TileStream::empty());
		}

		if bbox_dst.level <= self.level_base {
			log::trace!("get_stream level <= level_base");
			return self.source.as_ref().get_tile_stream(bbox_dst).await;
		}

		// Use tile climbing for all upscaling - process in parallel
		let coords: Vec<TileCoord> = bbox_dst.into_iter_coords().collect();
		let self_arc = Arc::new(self.clone()); // Share Operation across tasks
		let enable_climbing = self.enable_climbing;
		let tile_format = self.metadata.tile_format;

		let get_tile = async move |coord_dst: TileCoord| -> Result<Option<Tile>> {
			let (coord_src, image_src) = match self_arc.find_tile(coord_dst, enable_climbing).await? {
				Some(t) => t,
				None => return Ok(None),
			};

			Ok(Some(Tile::from_image(
				extract_image(&image_src, coord_src, coord_dst)?,
				tile_format,
			)?))
		};

		let stream = TileStream::from_coord_vec_async(coords, move |coord_dst| {
			let get_tile = get_tile.clone();
			async move {
				match get_tile(coord_dst).await {
					Ok(Some(tile)) => Some((coord_dst, tile)),
					Ok(None) => None,
					Err(e) => {
						log::error!("Error processing tile {:?}: {:?}", coord_dst, e);
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
		source: Box<dyn TileSource>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSource>> {
		Operation::build(vpl_node, source, factory)
			.await
			.map(|op| Box::new(op) as Box<dyn TileSource>)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use rstest::rstest;
	use versatiles_core::{TileCoord, TileFormat};
	use versatiles_image::{DynamicImage, DynamicImageTraitConvert, DynamicImageTraitOperation, DynamicImageTraitTest};

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
		let mut tiles = op.get_tile_stream(coord).await.unwrap().to_vec().await;
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
			VPLNode::try_from_str("raster_overscale level_base=2")?,
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

	#[tokio::test]
	async fn test_overscale_without_climbing() -> Result<()> {
		let image = make_gradient_image(3);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		// Build with climbing disabled (default)
		let op = Operation::build(
			VPLNode::try_from_str("raster_overscale level_base=2")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await?;

		// Should work for tiles at level_base (z=2)
		let coord_base = TileCoord::new(2, 0, 0)?.to_tile_bbox();
		let tiles = op.get_tile_stream(coord_base).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should return tile at level_base");

		// Should work for tiles above level_base (extracted from level_base)
		let coord_high = TileCoord::new(3, 0, 0)?.to_tile_bbox();
		let tiles = op.get_tile_stream(coord_high).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should extract from level_base for high zoom");

		// Multiple high-zoom tiles should reuse cached base tile
		let coord_high2 = TileCoord::new(3, 1, 0)?.to_tile_bbox();
		let tiles2 = op.get_tile_stream(coord_high2).await?.to_vec().await;
		assert_eq!(tiles2.len(), 1, "Should also work for adjacent tile");

		Ok(())
	}

	#[tokio::test]
	async fn test_overscale_with_climbing_enabled() -> Result<()> {
		let image = make_gradient_image(3);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		// Build with climbing ENABLED
		let op = Operation::build(
			VPLNode::try_from_str("raster_overscale level_base=2 enable_climbing=true")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await?;

		// Should work with climbing enabled
		let coord = TileCoord::new(3, 0, 0)?.to_tile_bbox();
		let tiles = op.get_tile_stream(coord).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should return tile with climbing enabled");

		Ok(())
	}
}
