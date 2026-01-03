use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::*;
use versatiles_derive::context;
use versatiles_image::traits::*;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Adjust brightness, contrast and gamma of raster tiles.
struct Args {
	/// Brightness adjustment, between -255 and 255. Defaults to 0.0 (no change).
	brightness: Option<f32>,
	/// Contrast adjustment, between 0 and infinity. Defaults to 1.0 (no change).
	contrast: Option<f32>,
	/// Gamma adjustment, between 0 and infinity. Defaults to 1.0 (no change).
	gamma: Option<f32>,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn TileSource>,
	brightness: f32,
	contrast: f32,
	gamma: f32,
}

impl Operation {
	#[context("Building raster_levels operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		Ok(Self {
			brightness: args.brightness.unwrap_or(0.0),
			contrast: args.contrast.unwrap_or(1.0),
			gamma: args.gamma.unwrap_or(1.0),
			source,
		})
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_levels", self.source.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		self.source.metadata()
	}

	fn tilejson(&self) -> &TileJSON {
		self.source.tilejson()
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {:?}", bbox);

		let contrast = self.contrast / 255.0;
		let brightness = self.brightness / 255.0;
		let gamma = self.gamma;
		Ok(self
			.source
			.get_tile_stream(bbox)
			.await?
			.map_item_parallel(move |mut tile| {
				tile.as_image_mut()?.mut_color_values(|v| {
					let v = ((v as f32 - 127.5) * contrast + 0.5 + brightness).powf(gamma) * 255.0;
					v.round().clamp(0.0, 255.0) as u8
				});
				Ok(tile)
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
		"raster_levels"
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
	use crate::helpers::dummy_image_source::DummyImageSource;
	use rstest::rstest;

	#[rstest]
	#[case::no_change(&[102], 0.0, 1.0, 1.0, &[102])]
	#[case::no_change(&[102,119], 0.0, 1.0, 1.0, &[102,119])]
	#[case::no_change(&[102,119,136], 0.0, 1.0, 1.0, &[102,119,136])]
	#[case::no_change(&[102,119,136,153], 0.0, 1.0, 1.0, &[102,119,136,153])]
	#[case::alpha_does_not_change(&[102], 20.0, 1.1, 0.9, &[129])]
	#[case::alpha_does_not_change(&[102,119], 20.0, 1.1, 0.9, &[129,119])]
	#[case::alpha_does_not_change(&[102,119,136], 20.0, 1.1, 0.9, &[129,147,165])]
	#[case::alpha_does_not_change(&[102,119,136,153], 20.0, 1.1, 0.9, &[129,147,165,153])]
	#[case::medium(&[51,119,170], 0.0, 1.0, 1.0, &[51,119,170])]
	#[case::brightness_dec(&[51,119,170], -100.0, 1.0, 1.0, &[0,19,70])]
	#[case::brightness_inc(&[51,119,170], 100.0, 1.0, 1.0, &[151,219,255])]
	#[case::contrast_dec(&[51,119,170], 0.0, 0.5, 1.0, &[89,123,149])]
	#[case::contrast_inc(&[51,119,170], 0.0, 2.0, 1.0, &[0,111,213])]
	#[case::gamma_dec(&[51,119,170], 0.0, 1.0, 0.5, &[114,174,208])]
	#[case::gamma_inc(&[51,119,170], 0.0, 1.0, 2.0, &[10,56,113])]
	#[tokio::test]
	async fn color_change_test(
		#[case] color_in: &[u8],
		#[case] brightness: f32,
		#[case] contrast: f32,
		#[case] gamma: f32,
		#[case] color_out: &[u8],
	) -> Result<()> {
		let op = Operation {
			source: Box::new(DummyImageSource::from_color(color_in, 4, TileFormat::PNG, None).unwrap()),
			brightness,
			contrast,
			gamma,
		};
		let mut tiles = op
			.get_tile_stream(TileBBox::from_min_and_max(8, 56, 56, 56, 56)?)
			.await?
			.to_vec()
			.await;
		assert_eq!(tiles.len(), 1);
		assert_eq!(tiles[0].1.as_image()?.average_color(), color_out);
		Ok(())
	}

	#[rstest]
	#[case(   0, 1.0, 1.0, [ 63, 157, 249])]
	#[case(   0, 0.5, 1.0, [ 95, 142, 189])]
	#[case(-127, 0.5, 1.0, [  0,  15,  62])]
	#[case(-127, 0.5, 1.5, [  0,   4,  30])]
	#[tokio::test]
	async fn test_raster_levels(
		#[case] brightness: i16,
		#[case] contrast: f32,
		#[case] gamma: f32,
		#[case] expected_color: [u8; 3],
	) -> Result<()> {
		let factory = PipelineFactory::new_dummy();

		let op = factory
			.operation_from_vpl(&format!(
				"from_debug format=png | raster_flatten color=[50,150,250] | raster_levels brightness={brightness} contrast={contrast} gamma={gamma}"
			))
			.await?;

		let bbox = TileCoord::new(3, 2, 1)?.to_tile_bbox();
		let adj = op.get_tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
		assert_eq!(adj.average_color(), expected_color);
		Ok(())
	}
}
