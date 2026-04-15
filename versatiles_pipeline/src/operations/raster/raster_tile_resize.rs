use crate::{PipelineFactory, helpers::tile_resize::TileResizeCore, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitOperation;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Convert the size of tiles by splitting or merging them to a width of 256px or 512px.
pub struct Args {
	/// Target tile size in pixels.
	/// A value of `256` expects source tiles of 512px, which will be split into four 256px output tiles at the next higher zoom level. Level 0 is downscaled instead.
	/// A value of `512` expects source tiles measuring 256px, which will be merged into 512px output tiles at the next lower zoom level.
	pub tile_size: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Operation {
	core: TileResizeCore,
}

impl Operation {
	#[context("Building raster_tile_resize operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let tile_size = args.tile_size.ok_or_else(|| anyhow::anyhow!("tile_size is required"))?;
		let core = TileResizeCore::new(source, tile_size, Arc::new(|img| img.scaled_down(2)))?;
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
		SourceType::new_processor("raster_tile_resize", self.core.source.as_ref().source_type())
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		self.core.tile_stream(bbox)
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.core.tile_coord_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("raster_tile_resize", Args, Operation);

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::factory::OperationFactoryTrait;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use versatiles_core::{TileBBox, TileCoord, TileFormat, TilePyramid};
	use versatiles_image::{DynamicImage, GenericImageView, traits::DynamicImageTraitConvert};

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "raster_tile_resize");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("tile_size"));
	}

	#[tokio::test]
	async fn test_build_rejects_same_tile_size() {
		let source = DummyImageSource::from_color(&[128, 128, 128], 256, TileFormat::PNG, None).unwrap();
		let result = TileResizeCore::new(Box::new(source), 256, Arc::new(|img| img.scaled_down(2)));
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_build_rejects_invalid_tile_size() {
		let source = DummyImageSource::from_color(&[128, 128, 128], 256, TileFormat::PNG, None).unwrap();
		let result = TileResizeCore::new(Box::new(source), 1024, Arc::new(|img| img.scaled_down(2)));
		assert!(result.is_err());
	}

	fn make_512_gradient_source() -> DummyImageSource {
		let image = DynamicImage::from_fn(512, 512, |x, y| [x as u8, y as u8, 255u8.wrapping_sub(x as u8)]);
		let pyramid = TilePyramid::new_full_up_to(4);
		let mut source = DummyImageSource::from_image(image, TileFormat::PNG, Some(pyramid)).unwrap();
		source.tilejson_mut().set_tile_size(512).unwrap();
		source
	}

	fn make_256_colored_source() -> DummyImageSource {
		let tile_fn = move |coord: &TileCoord| -> Option<Tile> {
			let (r, g, b) = match (coord.x % 2, coord.y % 2) {
				(0, 0) => (255u8, 0, 0),
				(1, 0) => (0, 255, 0),
				(0, 1) => (0, 0, 255),
				(1, 1) => (255, 255, 0),
				_ => unreachable!(),
			};
			let image = DynamicImage::from_fn(256, 256, |_, _| [r, g, b]);
			Some(Tile::from_image(image, TileFormat::PNG).unwrap())
		};
		let pyramid = TilePyramid::new_full_up_to(8);
		let mut source = DummyImageSource::new(tile_fn, TileFormat::PNG, Some(pyramid)).unwrap();
		source.tilejson_mut().set_tile_size(256).unwrap();
		source
	}

	fn make_op(source: DummyImageSource, tartile_size: u32) -> Result<Operation> {
		let core = TileResizeCore::new(Box::new(source), tartile_size, Arc::new(|img| img.scaled_down(2)))?;
		Ok(Operation { core })
	}

	#[tokio::test]
	async fn test_split_metadata() -> Result<()> {
		let op = make_op(make_512_gradient_source(), 256)?;

		assert_eq!(op.tilejson().tile_size.unwrap().size(), 256);
		let pyramid = &op.metadata().bbox_pyramid;
		assert!(!pyramid.level(0).is_empty());
		assert!(!pyramid.level(5).is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_merge_metadata() -> Result<()> {
		let op = make_op(make_256_colored_source(), 512)?;

		assert_eq!(op.tilejson().tile_size.unwrap().size(), 512);
		let pyramid = &op.metadata().bbox_pyramid;
		assert!(!pyramid.level(7).is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_split_correctness() -> Result<()> {
		let op = make_op(make_512_gradient_source(), 256)?;

		let bbox = TileBBox::new_full(1)?;
		let tiles: Vec<_> = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 4);

		for (coord, mut tile) in tiles {
			let image = tile.as_image()?;
			assert_eq!(image.width(), 256);
			assert_eq!(image.height(), 256);

			let px = image.get_pixel(10, 10);
			match (coord.x, coord.y) {
				(0 | 1, 0 | 1) => {
					assert_eq!(px[0], 10);
					assert_eq!(px[1], 10);
				}
				_ => panic!("unexpected coord"),
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_split_level0_downscale() -> Result<()> {
		let op = make_op(make_512_gradient_source(), 256)?;

		let bbox = TileBBox::new_full(0)?;
		let tiles: Vec<_> = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let (coord, mut tile) = tiles.into_iter().next().unwrap();
		assert_eq!(coord.level, 0);
		let image = tile.as_image()?;
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		Ok(())
	}

	#[tokio::test]
	async fn test_merge_correctness() -> Result<()> {
		let op = make_op(make_256_colored_source(), 512)?;

		let bbox = TileBBox::new_full(0)?;
		let tiles: Vec<_> = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let (_coord, mut tile) = tiles.into_iter().next().unwrap();
		let image = tile.as_image()?;
		assert_eq!(image.width(), 512);
		assert_eq!(image.height(), 512);

		let tl = image.get_pixel(10, 10);
		assert_eq!(tl[0], 255);
		assert_eq!(tl[1], 0);
		assert_eq!(tl[2], 0);

		let tr = image.get_pixel(266, 10);
		assert_eq!(tr[0], 0);
		assert_eq!(tr[1], 255);
		assert_eq!(tr[2], 0);

		let bl = image.get_pixel(10, 266);
		assert_eq!(bl[0], 0);
		assert_eq!(bl[1], 0);
		assert_eq!(bl[2], 255);

		let br = image.get_pixel(266, 266);
		assert_eq!(br[0], 255);
		assert_eq!(br[1], 255);
		assert_eq!(br[2], 0);

		Ok(())
	}

	#[tokio::test]
	async fn test_merge_missing_children() -> Result<()> {
		let tile_fn = move |coord: &TileCoord| -> Option<Tile> {
			if !coord.x.is_multiple_of(2) {
				return None;
			}
			let image = DynamicImage::from_fn(256, 256, |_, _| [200u8, 100, 50]);
			Some(Tile::from_image(image, TileFormat::PNG).unwrap())
		};
		let pyramid = TilePyramid::new_full_up_to(4);
		let mut source = DummyImageSource::new(tile_fn, TileFormat::PNG, Some(pyramid)).unwrap();
		source.tilejson_mut().set_tile_size(256).unwrap();

		let op = make_op(source, 512)?;

		let bbox = TileBBox::new_full(0)?;
		let tiles: Vec<_> = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let (_coord, mut tile) = tiles.into_iter().next().unwrap();
		let image = tile.as_image()?;
		assert_eq!(image.width(), 512);
		assert_eq!(image.height(), 512);

		let left = image.get_pixel(10, 10);
		assert_eq!(left[0], 200);
		Ok(())
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let op = make_op(make_512_gradient_source(), 256)?;
		assert!(op.source_type().to_string().contains("raster_tile_resize"));
		Ok(())
	}

	#[tokio::test]
	async fn test_outside_bbox_returns_empty() -> Result<()> {
		let op = make_op(make_512_gradient_source(), 256)?;

		let bbox = TileBBox::from_min_and_max(20, 1000, 1000, 1000, 1000)?;
		let tiles: Vec<_> = op.tile_stream(bbox).await?.to_vec().await;
		assert!(tiles.is_empty());
		Ok(())
	}
}
