use crate::{PipelineFactory, helpers::overview::OverviewCore, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TileStream};
use versatiles_image::traits::DynamicImageTraitOperation;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generate lower-zoom overview tiles by downscaling from a base zoom level.
struct Args {
	/// use this zoom level to build the overview. Defaults to the maximum zoom level of the source.
	level: Option<u8>,
}

#[derive(Debug)]
struct Operation {
	core: OverviewCore,
}

impl Operation {
	#[allow(clippy::unused_async)] // must be async for the factory macro
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let core = OverviewCore::new(source, args.level, Arc::new(|img| img.get_scaled_down(2)))?;

		Ok(Self { core })
	}
}

#[async_trait]
impl TileSource for Operation {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.core.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.core.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_overview", self.core.source.source_type())
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("raster_overview::get_tile_stream {bbox:?}");
		self.core.get_tile_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("raster_overview", Args, Operation);

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::factory::OperationFactoryTrait;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use imageproc::image::{DynamicImage, GenericImage, GenericImageView, Rgba};
	use versatiles_core::{GeoBBox, TileBBoxPyramid, TileCoord, TileFormat};

	async fn make_operation(tile_size: u32, level_base: u8) -> Operation {
		let pyramid = TileBBoxPyramid::from_geo_bbox(
			level_base,
			level_base,
			&GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap(),
		);

		return Operation::build(
			VPLNode::try_from_str(&format!("raster_overview level={level_base}")).unwrap(),
			Box::new(DummyImageSource::from_color(&[255, 0, 0], tile_size, TileFormat::PNG, Some(pyramid)).unwrap()),
			&PipelineFactory::new_dummy(),
		)
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn get_tile_stream_at_base_populates_cache() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata();
		let level_bbox = metadata.bbox_pyramid.get_level_bbox(6);
		let bbox = TileBBox::from_min_and_size(6, level_bbox.x_min()?, level_bbox.y_min()?, 1, 1)?;

		// Fetch at base level — should populate the cache with scaled-down entries
		let tiles = op.get_tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		// Cache should now contain entries for the base-level block
		assert!(
			!op.core.cache.is_empty(),
			"cache should be populated after base-level fetch"
		);

		Ok(())
	}

	#[tokio::test]
	async fn get_tile_stream_builds_lower_zoom_from_cache() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata().clone();
		let level_bbox = metadata.bbox_pyramid.get_level_bbox(6);

		// First, fetch all base-level tiles to populate the cache
		let base_bbox = *level_bbox;
		let _base_tiles = op.get_tile_stream(base_bbox).await?.to_vec().await;
		assert!(!op.core.cache.is_empty(), "cache should be populated");

		// Now fetch at level 5 — should compose from cached half-size images
		let lvl5_bbox = metadata.bbox_pyramid.get_level_bbox(5);
		let tiles_lvl5 = op.get_tile_stream(*lvl5_bbox).await?.to_vec().await;
		// Should produce at least one tile at level 5
		assert!(!tiles_lvl5.is_empty(), "should produce tiles at level 5 from cache");

		for (coord, _tile) in &tiles_lvl5 {
			assert_eq!(coord.level, 5);
		}

		Ok(())
	}

	/// Helper: create a solid half-size RGBA image where every pixel has the same color.
	fn solid_half(size: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
		let color = Rgba([r, g, b, a]);
		let mut img = DynamicImage::new_rgba8(size, size);
		for y in 0..size {
			for x in 0..size {
				img.put_pixel(x, y, color);
			}
		}
		img
	}

	#[tokio::test]
	async fn build_images_from_cache_composes_quadrants_with_correct_pixels() -> Result<()> {
		let op = make_operation(256, 6).await;
		let half_size = op.core.tile_size / 2; // 128

		// Manually populate the cache with known half-size (128×128) images for a 32×32 block at level 6.
		// Each tile at (x, y) gets a unique solid color: R=x, G=y, B=42, A=255.
		let block_key = TileCoord::new(6, 0, 0)?;
		let mut entries: Vec<(TileCoord, Option<DynamicImage>)> = Vec::new();
		for y in 0u32..32 {
			for x in 0u32..32 {
				let coord = TileCoord::new(6, x, y)?;
				entries.push((coord, Some(solid_half(half_size, x as u8, y as u8, 42, 255))));
			}
		}
		op.core.cache.insert(block_key, entries);

		// Request composed images at level 5 for a 2×2 bbox
		let bbox_lvl5 = TileBBox::from_min_and_size(5, 0, 0, 2, 2)?;
		let result = op.core.build_images_from_cache(bbox_lvl5).await?;
		let items: Vec<_> = result.into_iter().filter(|(_, img)| img.is_some()).collect();
		assert!(!items.is_empty());

		for (coord, img_opt) in items {
			assert_eq!(coord.level, 5);
			let img = img_opt.unwrap();
			assert_eq!(img.dimensions(), (256, 256));

			// Level-5 tile at (x0, y0) is composed from level-6 children placed in quadrants:
			//   (2*x0,   2*y0)   → top-left     (0..128, 0..128)
			//   (2*x0+1, 2*y0)   → top-right    (128..256, 0..128)
			//   (2*x0,   2*y0+1) → bottom-left  (0..128, 128..256)
			//   (2*x0+1, 2*y0+1) → bottom-right (128..256, 128..256)
			let x0 = coord.x as u8;
			let y0 = coord.y as u8;
			assert_eq!(img.get_pixel(0, 0).0, [x0 * 2, y0 * 2, 42, 255]);
			assert_eq!(img.get_pixel(128, 0).0, [x0 * 2 + 1, y0 * 2, 42, 255]);
			assert_eq!(img.get_pixel(0, 128).0, [x0 * 2, y0 * 2 + 1, 42, 255]);
			assert_eq!(img.get_pixel(128, 128).0, [x0 * 2 + 1, y0 * 2 + 1, 42, 255]);
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let op = make_operation(256, 6).await;
		let source_type = op.source_type();
		assert!(source_type.to_string().contains("raster_overview"));
		Ok(())
	}

	#[tokio::test]
	async fn test_metadata_and_tilejson() -> Result<()> {
		let op = make_operation(256, 6).await;
		// metadata and tilejson should be available
		let metadata = op.metadata();
		// After building overview, pyramid should extend to level 0
		assert!(metadata.bbox_pyramid.get_level_min().is_some());
		let _tilejson = op.tilejson();
		Ok(())
	}

	#[tokio::test]
	async fn test_get_tile_stream_at_base_level() -> Result<()> {
		let op = make_operation(256, 6).await;
		// Request tiles at base level within the pyramid bbox
		// The GeoBBox(2.224, 48.815, 2.47, 48.903) at level 6 covers tile (33, 22)
		let metadata = op.metadata();
		let level_bbox = metadata.bbox_pyramid.get_level_bbox(6);
		let bbox = TileBBox::from_min_and_size(6, level_bbox.x_min()?, level_bbox.y_min()?, 1, 1)?;
		let tiles = op.get_tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn test_level_above_base_passthrough() -> Result<()> {
		let op = make_operation(256, 6).await;
		// Request at level above level_base should pass through to source
		// Use coordinates that would be valid children of the base level bbox
		let metadata = op.metadata();
		let level_bbox = metadata.bbox_pyramid.get_level_bbox(6);
		let bbox = TileBBox::from_min_and_size(7, level_bbox.x_min()? * 2, level_bbox.y_min()? * 2, 1, 1)?;
		let tiles = op.get_tile_stream(bbox).await?.to_vec().await;
		// May or may not have tiles depending on source, but should not error
		assert!(tiles.len() <= 1);
		Ok(())
	}

	#[test]
	fn test_factory_get_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.get_tag_name(), "raster_overview");
	}

	#[test]
	fn test_factory_get_docs() {
		let factory = Factory {};
		let docs = factory.get_docs();
		assert!(docs.contains("level"));
	}

	#[tokio::test]
	async fn test_default_parameters() -> Result<()> {
		// Test with default parameters (no level or tile_size specified)
		let pyramid = TileBBoxPyramid::from_geo_bbox(6, 6, &GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap());

		let op = Operation::build(
			VPLNode::try_from_str("raster_overview").unwrap(),
			Box::new(DummyImageSource::from_color(&[255, 0, 0], 512, TileFormat::PNG, Some(pyramid)).unwrap()),
			&PipelineFactory::new_dummy(),
		)
		.await?;

		// Should default to level_base from source max and tile_size 512
		assert_eq!(op.core.tile_size, 512);
		assert_eq!(op.core.level_base, 6);
		Ok(())
	}
}
