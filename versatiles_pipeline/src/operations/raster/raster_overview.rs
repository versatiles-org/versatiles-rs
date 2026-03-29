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

	async fn get_tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.core.get_tile_coord_stream(bbox).await
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
	use versatiles_core::{Blob, GeoBBox, TileBBoxPyramid, TileCoord, TileFormat};

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
		let mut entries: Vec<(TileCoord, Option<Blob>)> = Vec::new();
		for y in 0u32..32 {
			for x in 0u32..32 {
				let coord = TileCoord::new(6, x, y)?;
				let img = solid_half(half_size, x as u8, y as u8, 42, 255);
				let blob = versatiles_image::format::png::encode(&img, Some(0))?;
				entries.push((coord, Some(blob)));
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

	#[tokio::test]
	async fn cache_bytes_tracks_insert_and_remove() -> Result<()> {
		use std::sync::atomic::Ordering;

		let op = make_operation(256, 6).await;
		assert_eq!(op.core.cache_bytes.load(Ordering::Relaxed), 0);

		// Fetch at base level to populate cache
		let metadata = op.metadata().clone();
		let base_bbox = *metadata.bbox_pyramid.get_level_bbox(6);
		let _tiles = op.get_tile_stream(base_bbox).await?.to_vec().await;

		// cache_bytes should be non-zero after populating
		let bytes_after_insert = op.core.cache_bytes.load(Ordering::Relaxed);
		assert!(bytes_after_insert > 0, "cache_bytes should increase after insert");

		// Walk all the way down to level 0 to fully drain the cache
		for level in (0..6).rev() {
			let lvl_bbox = metadata.bbox_pyramid.get_level_bbox(level);
			let _tiles = op.get_tile_stream(*lvl_bbox).await?.to_vec().await;
		}

		// After draining everything, cache_bytes should be 0
		let bytes_after_drain = op.core.cache_bytes.load(Ordering::Relaxed);
		assert_eq!(bytes_after_drain, 0, "cache_bytes should be 0 after full drain");

		Ok(())
	}

	#[tokio::test]
	async fn cache_is_drained_after_full_overview_build() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata().clone();

		// Fetch all base-level tiles
		let base_bbox = *metadata.bbox_pyramid.get_level_bbox(6);
		let _base_tiles = op.get_tile_stream(base_bbox).await?.to_vec().await;
		assert!(!op.core.cache.is_empty(), "cache should be populated");

		// Walk down through all zoom levels to drain the cache
		for level in (0..6).rev() {
			let lvl_bbox = metadata.bbox_pyramid.get_level_bbox(level);
			let _tiles = op.get_tile_stream(*lvl_bbox).await?.to_vec().await;
		}

		// After consuming all levels the cache should be empty
		assert!(
			op.core.cache.is_empty(),
			"cache should be fully drained after building all levels"
		);

		Ok(())
	}

	#[tokio::test]
	async fn build_images_from_cache_with_empty_cache() -> Result<()> {
		let op = make_operation(256, 6).await;

		// Request from cache without populating it — should return empty/transparent tiles
		let bbox = TileBBox::from_min_and_size(5, 0, 0, 1, 1)?;
		let result = op.core.build_images_from_cache(bbox).await?;
		let items: Vec<_> = result.into_iter().filter(|(_, img)| img.is_some()).collect();
		assert!(items.is_empty(), "empty cache should produce no composed images");

		Ok(())
	}

	#[tokio::test]
	async fn build_images_from_cache_with_none_entries() -> Result<()> {
		let op = make_operation(256, 6).await;
		let half_size = op.core.tile_size / 2;

		// Insert entries where some are None (missing tiles)
		let block_key = TileCoord::new(6, 0, 0)?;
		let mut entries: Vec<(TileCoord, Option<Blob>)> = Vec::new();
		for y in 0u32..16 {
			for x in 0u32..16 {
				let coord = TileCoord::new(6, x, y)?;
				// Only populate even x,y tiles; odd ones are None
				let blob = if x % 2 == 0 && y % 2 == 0 {
					Some(versatiles_image::format::png::encode(
						&solid_half(half_size, x as u8, y as u8, 42, 255),
						Some(0),
					)?)
				} else {
					None
				};
				entries.push((coord, blob));
			}
		}
		op.core.cache.insert(block_key, entries);

		let bbox = TileBBox::from_min_and_size(5, 0, 0, 8, 8)?;
		let result = op.core.build_images_from_cache(bbox).await?;

		// Should still produce some composed tiles without errors
		let items: Vec<_> = result.into_iter().collect();
		assert!(!items.is_empty(), "should produce results even with None entries");

		Ok(())
	}

	#[test]
	fn estimate_entry_bytes_with_blobs_and_nones() {
		use crate::helpers::overview::estimate_entry_bytes;

		let coord = TileCoord::new(6, 0, 0).unwrap();

		// None entries = 16 bytes each
		let entries_none: Vec<(TileCoord, Option<Blob>)> = vec![(coord, None), (coord, None)];
		assert_eq!(estimate_entry_bytes(&entries_none), 32);

		// Blob with known length
		let blob = Blob::from(vec![0u8; 100]);
		let entries_blob: Vec<(TileCoord, Option<Blob>)> = vec![(coord, Some(blob))];
		assert_eq!(estimate_entry_bytes(&entries_blob), 100);

		// Mixed
		let blob2 = Blob::from(vec![0u8; 50]);
		let entries_mixed: Vec<(TileCoord, Option<Blob>)> = vec![(coord, None), (coord, Some(blob2))];
		assert_eq!(estimate_entry_bytes(&entries_mixed), 16 + 50);

		// Empty
		let entries_empty: Vec<(TileCoord, Option<Blob>)> = vec![];
		assert_eq!(estimate_entry_bytes(&entries_empty), 0);
	}

	#[tokio::test]
	async fn full_pipeline_produces_tiles_at_every_level() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata().clone();

		// Fetch base level
		let base_bbox = *metadata.bbox_pyramid.get_level_bbox(6);
		let base_tiles = op.get_tile_stream(base_bbox).await?.to_vec().await;
		assert!(!base_tiles.is_empty(), "base level should have tiles");

		// Walk every level from 5 down to 0 and verify tiles are produced
		for level in (0..6).rev() {
			let lvl_bbox = metadata.bbox_pyramid.get_level_bbox(level);
			let tiles = op.get_tile_stream(*lvl_bbox).await?.to_vec().await;
			assert!(!tiles.is_empty(), "level {level} should produce at least one tile");
			for (coord, _) in &tiles {
				assert_eq!(coord.level, level, "tile should be at level {level}");
			}
		}

		Ok(())
	}

	#[test]
	fn debug_format_includes_fields() {
		let rt = tokio::runtime::Runtime::new().unwrap();
		let op = rt.block_on(make_operation(256, 6));
		let debug = format!("{op:?}");
		assert!(debug.contains("level_base"), "debug should include level_base");
		assert!(debug.contains("tile_size"), "debug should include tile_size");
	}

	#[tokio::test]
	async fn tile_512_overview_produces_tiles() -> Result<()> {
		// Test with a different tile size (512) to ensure it's not hardcoded
		let op = make_operation(512, 4).await;
		assert_eq!(op.core.tile_size, 512);

		let metadata = op.metadata().clone();
		let base_bbox = *metadata.bbox_pyramid.get_level_bbox(4);
		let base_tiles = op.get_tile_stream(base_bbox).await?.to_vec().await;
		assert!(!base_tiles.is_empty());

		// Build level 3 from cache
		let lvl3_bbox = metadata.bbox_pyramid.get_level_bbox(3);
		let tiles = op.get_tile_stream(*lvl3_bbox).await?.to_vec().await;
		assert!(!tiles.is_empty(), "should produce overview tiles with tile_size=512");

		Ok(())
	}
}
