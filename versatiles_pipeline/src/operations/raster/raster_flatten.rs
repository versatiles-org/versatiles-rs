use crate::{PipelineFactory, helpers::pack_image_tile_stream, traits::*, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::{DynamicImage, Rgb};
use std::fmt::Debug;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_geometry::vector_tile::VectorTile;
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

	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		let color = self.color;
		Ok(self
			.source
			.get_image_stream(bbox)
			.await?
			.map_item_parallel(move |image| image.into_flattened(color)))
	}

	async fn get_blob_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		// todo: don't decompress and recompress tiles that are already flattened
		pack_image_tile_stream(self.get_image_stream(bbox).await, self.source.parameters())
	}

	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("Vector tiles are not supported in raster_flatten operations.");
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
