use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use imageproc::image::Rgb;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSourceTrait, TilesReaderParameters};
use versatiles_core::*;
use versatiles_derive::context;
use versatiles_image::traits::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Flattens (translucent) raster tiles onto a background
struct Args {
	/// background color to use for the flattened tiles, in RGB format. Defaults to white.
	color: Option<[u8; 3]>,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn TileSourceTrait>,
	color: Rgb<u8>,
}

impl Operation {
	#[context("Building raster_flatten operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSourceTrait>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSourceTrait,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		Ok(Self {
			color: Rgb(args.color.unwrap_or([255, 255, 255])),
			source,
		})
	}
}

#[async_trait]
impl TileSourceTrait for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_flatten", self.source.source_type())
	}

	fn parameters(&self) -> &TilesReaderParameters {
		self.source.parameters()
	}

	fn tilejson(&self) -> &TileJSON {
		self.source.tilejson()
	}

	fn traversal(&self) -> &Traversal {
		self.source.traversal()
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {:?}", bbox);

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
			}))
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
		source: Box<dyn TileSourceTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSourceTrait>> {
		Operation::build(vpl_node, source, factory)
			.await
			.map(|op| Box::new(op) as Box<dyn TileSourceTrait>)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::PipelineFactory;

	#[tokio::test]
	async fn test_raster_flatten() -> Result<()> {
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
}
