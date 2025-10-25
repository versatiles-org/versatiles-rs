use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::Rgb;
use std::fmt::Debug;
use versatiles_container::Tile;
use versatiles_core::*;
use versatiles_image::traits::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Flattens (translucent) raster tiles onto a background
struct Args {
	/// background color to use for the flattened tiles, in RGB format. Defaults to white.
	color: Option<[u8; 3]>,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn OperationTrait>,
	color: Rgb<u8>,
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

			Ok(Box::new(Self {
				color: Rgb(args.color.unwrap_or([255, 255, 255])),
				source,
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn parameters(&self) -> &TilesReaderParameters {
		self.source.parameters()
	}

	fn tilejson(&self) -> &TileJSON {
		self.source.tilejson()
	}

	fn traversal(&self) -> &Traversal {
		self.source.traversal()
	}

	async fn get_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {:?}", bbox);

		let color = self.color;
		Ok(self.source.get_stream(bbox).await?.map_item_parallel(move |mut tile| {
			if tile.as_image()?.has_alpha() {
				let format = tile.format();
				let image = tile.into_image()?;
				let image = image.into_flattened(color)?;
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
		source: Box<dyn OperationTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, source, factory).await
	}
}

#[cfg(test)]
mod tests {}
