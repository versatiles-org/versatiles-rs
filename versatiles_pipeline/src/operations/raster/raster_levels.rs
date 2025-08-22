use crate::{PipelineFactory, helpers::pack_image_tile_stream, traits::*, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::BoxFuture;
use imageproc::image::DynamicImage;
use std::fmt::Debug;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::traits::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Flattens (translucent) raster tiles onto a background
struct Args {
	/// Brightness adjustment for the flattened tiles. Defaults to 0.0 (no change).
	brightness: Option<f32>,
	/// Contrast adjustment for the flattened tiles. Defaults to 1.0 (no change).
	contrast: Option<f32>,
	/// Gamma adjustment for the flattened tiles. Defaults to 1.0 (no change).
	gamma: Option<f32>,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn OperationTrait>,
	brightness: f32,
	contrast: f32,
	gamma: f32,
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
				brightness: args.brightness.unwrap_or(0.0),
				contrast: args.contrast.unwrap_or(1.0),
				gamma: args.gamma.unwrap_or(1.0),
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
		let contrast = self.contrast / 255.0;
		let brightness = self.brightness / 255.0;
		let gamma = self.gamma;
		Ok(self
			.source
			.get_image_stream(bbox)
			.await?
			.map_item_parallel(move |mut image| {
				image.mut_color_values(|v| {
					let v = ((v as f32 - 127.5) * contrast + 0.5 + brightness).powf(gamma) * 255.0;
					v.clamp(0.0, 255.0) as u8
				});
				Ok(image)
			}))
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		pack_image_tile_stream(self.get_image_stream(bbox).await, self.source.parameters())
	}

	async fn get_vector_stream(&self, _bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		bail!("Vector tiles are not supported in raster_levels operations.");
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"raster_levels"
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
