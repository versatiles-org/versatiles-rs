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
					v.round().clamp(0.0, 255.0) as u8
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
mod tests {
	use super::*;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use rstest::rstest;

	#[rstest]
	#[case::no_change("6", [0.0, 1.0, 1.0], &[102])]
	#[case::no_change("67", [0.0, 1.0, 1.0], &[102, 119])]
	#[case::no_change("678", [0.0, 1.0, 1.0], &[102, 119, 136])]
	#[case::no_change("6789", [0.0, 1.0, 1.0], &[102, 119, 136, 153])]
	#[case::alpha_does_not_change("6", [20.0, 1.1, 0.9], &[129])]
	#[case::alpha_does_not_change("67", [20.0, 1.1, 0.9], &[129, 119])]
	#[case::alpha_does_not_change("678", [20.0, 1.1, 0.9], &[129, 147, 165])]
	#[case::alpha_does_not_change("6789", [20.0, 1.1, 0.9], &[129, 147, 165, 153])]
	#[case::medium("37A", [0.0, 1.0, 1.0], &[51, 119, 170])]
	#[case::brightness_dec("37A", [-100.0, 1.0, 1.0], &[0, 19, 70])]
	#[case::brightness_inc("37A", [100.0, 1.0, 1.0], &[151, 219, 255])]
	#[case::contrast_dec("37A", [0.0, 0.5, 1.0], &[89, 123, 149])]
	#[case::contrast_inc("37A", [0.0, 2.0, 1.0], &[0, 111, 213])]
	#[case::gamma_dec("37A", [0.0, 1.0, 0.5], &[114, 174, 208])]
	#[case::gamma_inc("37A", [0.0, 1.0, 2.0], &[10, 56, 113])]
	#[tokio::test]
	async fn color_change_test(
		#[case] color_in: &str,
		#[case] parameters: [f32; 3],
		#[case] color_out: &[u8],
	) -> Result<()> {
		let op = Operation {
			source: Box::new(DummyImageSource::new(&format!("{color_in}.png"), None, 4).unwrap()),
			brightness: parameters[0],
			contrast: parameters[1],
			gamma: parameters[2],
		};
		let images = op
			.get_image_stream(TileBBox::from_boundaries(8, 56, 56, 56, 56)?)
			.await?
			.to_vec()
			.await;
		assert_eq!(images.len(), 1);
		assert_eq!(images[0].1.average_color(), color_out);
		Ok(())
	}
}
