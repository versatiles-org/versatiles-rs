use crate::{
	PipelineFactory,
	traits::{OperationFactoryTrait, TransformOperationFactoryTrait},
	vpl::VPLNode,
};
use anyhow::Result;
use async_trait::async_trait;
use imageproc::image::Rgb;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitOperation;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Flattens (translucent) raster tiles onto a background
struct Args {
	/// background color to use for the flattened tiles, in RGB format. Defaults to white.
	color: Option<[u8; 3]>,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn TileSource>,
	color: Rgb<u8>,
}

impl Operation {
	#[context("Building raster_flatten operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		Ok(Self {
			color: Rgb(args.color.unwrap_or([255, 255, 255])),
			source,
		})
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_flatten", self.source.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		self.source.metadata()
	}

	fn tilejson(&self) -> &TileJSON {
		self.source.tilejson()
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {bbox:?}");

		let color = self.color;
		Ok(self
			.source
			.get_tile_stream(bbox)
			.await?
			.map_item_parallel(move |mut tile| {
				if tile.as_image()?.has_alpha() {
					let format = tile.format();
					let image = tile.into_image()?.into_flattened(color)?;
					let tile = Tile::from_image(image, format)?;
					Ok(tile)
				} else {
					Ok(tile)
				}
			})
			.unwrap_results())
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"raster_flatten"
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
	use crate::PipelineFactory;
	use versatiles_core::TileCoord;
	use versatiles_image::DynamicImageTraitOperation;

	#[tokio::test]
	async fn test_raster_flatten_custom_color() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_flatten color=[255,127,0]")
			.await?;

		let bbox = TileCoord::new(2, 1, 1)?.to_tile_bbox();
		let image = op.get_tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
		assert_eq!(image.average_color(), [238, 119, 0]);

		let bbox = TileCoord::new(2, 2, 1)?.to_tile_bbox();
		let image = op.get_tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
		assert_eq!(image.average_color(), [254, 135, 16]);

		Ok(())
	}

	#[tokio::test]
	async fn test_raster_flatten_default_color() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// No color specified, defaults to white [255, 255, 255]
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_flatten")
			.await?;

		let bbox = TileCoord::new(2, 1, 1)?.to_tile_bbox();
		let image = op.get_tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
		// With white background, the average should be different from custom orange
		let avg = image.average_color();
		assert_eq!(avg.len(), 3); // RGB output (no alpha)

		Ok(())
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_flatten")
			.await?;

		let source_type = op.source_type();
		assert!(source_type.to_string().contains("raster_flatten"));

		Ok(())
	}

	#[tokio::test]
	async fn test_metadata_and_tilejson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_flatten")
			.await?;

		// metadata and tilejson should be passed through from source
		let _metadata = op.metadata();
		let _tilejson = op.tilejson();

		Ok(())
	}

	#[test]
	fn test_factory_get_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.get_tag_name(), "raster_flatten");
	}

	#[test]
	fn test_factory_get_docs() {
		let factory = Factory {};
		let docs = factory.get_docs();
		assert!(docs.contains("color"));
	}
}
