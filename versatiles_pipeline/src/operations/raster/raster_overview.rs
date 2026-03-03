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
	/// Size of the tiles in pixels. Defaults to 512.
	tile_size: Option<u32>,
}

#[derive(Debug)]
struct Operation {
	core: OverviewCore,
}

impl Operation {
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let core = OverviewCore::new(
			source,
			args.level,
			args.tile_size,
			Arc::new(|img| img.get_scaled_down(2)),
		)
		.await?;

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
	use versatiles_core::{GeoBBox, TileBBoxMap, TileBBoxPyramid, TileCoord, TileFormat};

	async fn make_operation(tile_size: u32, level_base: u8) -> Operation {
		let pyramid = TileBBoxPyramid::from_geo_bbox(
			level_base,
			level_base,
			&GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap(),
		);

		return Operation::build(
			VPLNode::try_from_str(&format!("raster_overview level={level_base} tile_size={tile_size}")).unwrap(),
			Box::new(DummyImageSource::from_color(&[255, 0, 0], tile_size, TileFormat::PNG, Some(pyramid)).unwrap()),
			&PipelineFactory::new_dummy(),
		)
		.await
		.unwrap();
	}

	fn solid_rgba(size: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
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
	async fn add_images_to_cache_inserts_half_tiles_under_floored_key() -> Result<()> {
		let op = make_operation(2, 6).await;
		let bbox = TileBBox::from_min_and_max(6, 0, 0, 31, 31)?; // 32x32 block at base level
		let mut container = TileBBoxMap::new_default(bbox)?;
		// Populate with simple solid tiles (only a tiny subset to keep it cheap)
		for y in 0..bbox.height() {
			for x in 0..bbox.width() {
				let c = TileCoord::new(6, bbox.x_min()? + x, bbox.y_min()? + y)?;
				container.insert(c, Some(solid_rgba(2, x as u8, y as u8, 32, 255)))?;
			}
		}

		op.core.add_images_to_cache(&container).await?;

		// Cache key should be the floored corner of the container bbox at level 6
		let key = TileCoord::new(6, 0, 0)?;
		assert!(op.core.cache.contains_key(&key));

		let (_key, stored) = op.core.cache.remove(&key).expect("value stored");
		// Stored entries are (coord, Option<img>) with half-size images (1x1 at tile_size=2)
		assert!(!stored.is_empty());

		for (coord, img_opt) in stored {
			assert_eq!(coord.level, 6);
			assert_eq!(img_opt.unwrap().dimensions(), (1, 1));
		}

		Ok(())
	}

	#[tokio::test]
	async fn build_images_from_cache_composes_quadrants() -> Result<()> {
		let op = make_operation(2, 6).await;

		// Prepare cache content by adding a full 32x32 block at level 6
		let bbox_lvl6 = TileBBox::from_min_and_size(6, 0, 0, 32, 32)?;
		let mut cont6 = TileBBoxMap::new_default(bbox_lvl6)?;
		for y in 0..bbox_lvl6.height() {
			for x in 0..bbox_lvl6.width() {
				let c = TileCoord::new(6, x, y)?;
				cont6.insert(c, Some(solid_rgba(2, x as u8, y as u8, 0, 255)))?;
			}
		}
		op.core.add_images_to_cache(&cont6).await?;

		// Now request composed images at level 5 for a tiny bbox (2x2 tiles)
		let bbox_lvl5 = TileBBox::from_min_and_size(5, 0, 0, 2, 2)?;
		let result = op.core.build_images_from_cache(bbox_lvl5).await?;
		let items: Vec<_> = result.into_iter().collect();
		// We expect at least one composed tile present (others may be missing if cache quadrants absent)
		assert!(!items.is_empty());

		for (coord, img_opt) in items {
			assert_eq!(coord.level, 5);
			let img = img_opt.unwrap();
			assert_eq!(img.dimensions(), (2, 2));
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

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let op = make_operation(2, 6).await;
		let source_type = op.source_type();
		assert!(source_type.to_string().contains("raster_overview"));
		Ok(())
	}

	#[tokio::test]
	async fn test_metadata_and_tilejson() -> Result<()> {
		let op = make_operation(2, 6).await;
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
		assert!(docs.contains("tile_size"));
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
