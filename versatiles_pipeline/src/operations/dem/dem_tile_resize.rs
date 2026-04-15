use super::encoding::{resolve_encoding, to_tile_schema};
use crate::{PipelineFactory, helpers::tile_resize::TileResizeCore, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TileStream};
use versatiles_derive::context;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Convert DEM tile size between 256px and 512px by splitting or merging tiles.
///
/// Like raster_tile_resize, but uses 24-bit raw value averaging for downscaling
/// (level 0, 512→256) instead of channel-wise averaging.
pub struct Args {
	/// Target tile size in pixels. Must be 256 or 512.
	pub tile_size: Option<u32>,
	/// Override auto-detection of DEM encoding. Values: "mapbox", "terrarium".
	pub encoding: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Operation {
	core: TileResizeCore,
}

impl Operation {
	#[context("Building dem_tile_resize operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let tile_size = args.tile_size.ok_or_else(|| anyhow::anyhow!("tile_size is required"))?;

		let encoding = resolve_encoding(args.encoding.as_ref(), source.tilejson().tile_schema.as_ref())?;

		let mut core = TileResizeCore::new(source, tile_size, Arc::new(super::dem_overview::dem_scale_down))?;
		core.tilejson.tile_schema = Some(to_tile_schema(encoding));

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
		SourceType::new_processor("dem_tile_resize", self.core.source.as_ref().source_type())
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		self.core.tile_stream(bbox)
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.core.tile_coord_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("dem_tile_resize", Args, Operation);

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::factory::OperationFactoryTrait;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use imageproc::image::{DynamicImage, GenericImageView, Rgb, RgbImage};
	use versatiles_core::{TileBBox, TileCoord, TileFormat, TilePyramid, TileSchema};

	fn raw_to_rgb(v: u32) -> Rgb<u8> {
		Rgb([((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8])
	}

	fn rgb_to_raw(r: u8, g: u8, b: u8) -> u32 {
		(u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
	}

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "dem_tile_resize");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("tile_size"));
		assert!(docs.contains("encoding"));
	}

	#[tokio::test]
	async fn test_build_rejects_same_tile_size() {
		let source = DummyImageSource::from_color(&[128, 128, 128], 256, TileFormat::PNG, None).unwrap();
		let result = TileResizeCore::new(
			Box::new(source),
			256,
			Arc::new(super::super::dem_overview::dem_scale_down),
		);
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_build_fails_without_dem_schema() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_debug format=png | dem_tile_resize tile_size=256")
			.await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_build_with_encoding_override() -> Result<()> {
		let source = make_dem_512_source();
		let factory = PipelineFactory::new_dummy();
		let op = Operation::build(
			crate::vpl::VPLNode::try_from_str("dem_tile_resize tile_size=256 encoding=terrarium")?,
			Box::new(source),
			&factory,
		)
		.await?;
		assert!(op.source_type().to_string().contains("dem_tile_resize"));
		assert_eq!(op.tilejson().tile_schema, Some(TileSchema::RasterDEMTerrarium));
		Ok(())
	}

	fn make_dem_512_source() -> DummyImageSource {
		let raw_val = 100_000u32;
		let px = raw_to_rgb(raw_val);
		let mut img = RgbImage::new(512, 512);
		for y in 0..512 {
			for x in 0..512 {
				img.put_pixel(x, y, px);
			}
		}
		let image = DynamicImage::ImageRgb8(img);
		let pyramid = TilePyramid::new_full_up_to(4);
		let mut source = DummyImageSource::from_image(image, TileFormat::PNG, Some(pyramid)).unwrap();
		source.tilejson_mut().set_tile_size(512).unwrap();
		source.tilejson_mut().tile_schema = Some(TileSchema::RasterDEMMapbox);
		source
	}

	fn make_op(source: DummyImageSource, tartile_size: u32) -> Result<Operation> {
		let core = TileResizeCore::new(
			Box::new(source),
			tartile_size,
			Arc::new(super::super::dem_overview::dem_scale_down),
		)?;
		Ok(Operation { core })
	}

	#[tokio::test]
	async fn test_split_metadata() -> Result<()> {
		let mut op = make_op(make_dem_512_source(), 256)?;
		op.core.tilejson.tile_schema = Some(TileSchema::RasterDEMMapbox);

		assert_eq!(op.tilejson().tile_size.unwrap().size(), 256);
		let pyramid = &op.metadata().bbox_pyramid;
		assert!(!pyramid.level(0).is_empty());
		assert!(!pyramid.level(5).is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_split_level0_dem_downscale() -> Result<()> {
		let op = make_op(make_dem_512_source(), 256)?;

		let bbox = TileBBox::new_full(0)?;
		let tiles: Vec<_> = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let (_coord, mut tile) = tiles.into_iter().next().unwrap();
		let image = tile.as_image()?;
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);

		let px = image.get_pixel(0, 0);
		let raw = rgb_to_raw(px[0], px[1], px[2]);
		assert_eq!(raw, 100_000);
		Ok(())
	}

	#[tokio::test]
	async fn test_split_quadrants() -> Result<()> {
		let mut img = RgbImage::new(512, 512);
		for y in 0..512 {
			for x in 0..512 {
				let raw = match (x < 256, y < 256) {
					(true, true) => 50_000u32,
					(false, true) => 60_000,
					(true, false) => 70_000,
					(false, false) => 80_000,
				};
				img.put_pixel(x, y, raw_to_rgb(raw));
			}
		}
		let image = DynamicImage::ImageRgb8(img);
		let pyramid = TilePyramid::new_full_up_to(4);
		let mut source = DummyImageSource::from_image(image, TileFormat::PNG, Some(pyramid))?;
		source.tilejson_mut().set_tile_size(512)?;

		let op = make_op(source, 256)?;

		let bbox = TileBBox::new_full(1)?;
		let tiles: Vec<_> = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 4);

		for (coord, mut tile) in tiles {
			let image = tile.as_image()?;
			assert_eq!(image.width(), 256);
			let px = image.get_pixel(128, 128);
			let raw = rgb_to_raw(px[0], px[1], px[2]);
			let expected = match (coord.x, coord.y) {
				(0, 0) => 50_000u32,
				(1, 0) => 60_000,
				(0, 1) => 70_000,
				(1, 1) => 80_000,
				_ => panic!("unexpected coord"),
			};
			assert_eq!(raw, expected, "wrong value at coord ({}, {})", coord.x, coord.y);
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_merge_correctness() -> Result<()> {
		let tile_fn = move |coord: &TileCoord| -> Option<Tile> {
			let raw = match (coord.x % 2, coord.y % 2) {
				(0, 0) => 50_000u32,
				(1, 0) => 60_000,
				(0, 1) => 70_000,
				(1, 1) => 80_000,
				_ => unreachable!(),
			};
			let px = raw_to_rgb(raw);
			let mut img = RgbImage::new(256, 256);
			for y in 0..256 {
				for x in 0..256 {
					img.put_pixel(x, y, px);
				}
			}
			Some(Tile::from_image(DynamicImage::ImageRgb8(img), TileFormat::PNG).unwrap())
		};
		let pyramid = TilePyramid::new_full_up_to(8);
		let mut source = DummyImageSource::new(tile_fn, TileFormat::PNG, Some(pyramid))?;
		source.tilejson_mut().set_tile_size(256)?;

		let op = make_op(source, 512)?;

		let bbox = TileBBox::new_full(0)?;
		let tiles: Vec<_> = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let (_coord, mut tile) = tiles.into_iter().next().unwrap();
		let image = tile.as_image()?;
		assert_eq!(image.width(), 512);
		assert_eq!(image.height(), 512);

		let check = |x: u32, y: u32, expected_raw: u32| {
			let px = image.get_pixel(x, y);
			let raw = rgb_to_raw(px[0], px[1], px[2]);
			assert_eq!(raw, expected_raw, "wrong value at pixel ({x}, {y})");
		};
		check(10, 10, 50_000);
		check(266, 10, 60_000);
		check(10, 266, 70_000);
		check(266, 266, 80_000);

		Ok(())
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let op = make_op(make_dem_512_source(), 256)?;
		assert!(op.source_type().to_string().contains("dem_tile_resize"));
		Ok(())
	}
}
