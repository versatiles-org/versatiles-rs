use anyhow::Result;
use versatiles_container::{Tile, TileSource};
use versatiles_core::{TileCoord, bounded_number};
use versatiles_derive::context;
use versatiles_image::traits::DynamicImageTraitOperation;

use crate::{
	PipelineFactory,
	operations::transform::{TileTransform, TransformOp},
	vpl::VPLNode,
};

bounded_number! {
	/// An offset added to every colour channel, in 0..=255 terms.
	Brightness: f32, -255.0..=255.0;
	/// A factor applied around mid-grey. Zero would flatten every tile to one
	/// colour and a negative factor would invert it, so neither is accepted.
	Contrast: f32, greater_than 0.0;
	/// A gamma exponent. Zero divides by zero rather than being a mild edge.
	Gamma: f32, greater_than 0.0;
}

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Adjusts the brightness, contrast and gamma of raster tiles.
struct Args {
	/// Offset added to every channel. Defaults to `0.0`.
	#[vpl(default = "0.0")]
	brightness: Option<Brightness>,
	/// Factor applied around mid-grey. Defaults to `1.0`.
	#[vpl(default = "1.0")]
	contrast: Option<Contrast>,
	/// Gamma exponent. Defaults to `1.0`.
	#[vpl(default = "1.0")]
	gamma: Option<Gamma>,
}

#[derive(Debug)]
struct Operation {
	/// Pre-divided by 255 at build time: `run` is called once per tile.
	brightness: f32,
	contrast: f32,
	gamma: f32,
}

impl Operation {
	#[context("Building raster_levels operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<Operation>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let operation = Operation {
			brightness: args.brightness.map_or(0.0, Brightness::get) / 255.0,
			contrast: args.contrast.map_or(1.0, Contrast::get) / 255.0,
			gamma: args.gamma.map_or(1.0, Gamma::get),
		};
		Ok(TransformOp::new(source, operation, factory.runtime()))
	}
}

impl TileTransform for Operation {
	const TAG: &'static str = "raster_levels";

	fn run(&self, _coord: &TileCoord, mut tile: Tile) -> Result<Option<Tile>> {
		let (contrast, brightness, gamma) = (self.contrast, self.brightness, self.gamma);
		#[expect(
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "clamped to 0..=255 before the cast"
		)]
		tile.as_image_mut()?.map_color_values(|v| {
			(((f32::from(v) - 127.5) * contrast + 0.5 + brightness).powf(gamma) * 255.0)
				.round()
				.clamp(0.0, 255.0) as u8
		});
		Ok(Some(tile))
	}
}

crate::operations::macros::define_transform_factory!("raster_levels", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use rstest::rstest;
	use versatiles_container::TilesRuntime;
	use versatiles_core::{TileBBox, TileFormat};
	use versatiles_image::DynamicImageTraitOperation;

	use super::*;
	use crate::{factory::OperationFactoryTrait, helpers::dummy_image_source::DummyImageSource};

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
		let op = TransformOp::new(
			Box::new(DummyImageSource::from_color(color_in, 4, TileFormat::PNG, None).unwrap()),
			Operation {
				brightness: brightness / 255.0,
				contrast: contrast / 255.0,
				gamma,
			},
			TilesRuntime::new_silent(),
		);
		let mut tiles = op
			.tile_stream(TileBBox::from_min_and_max(8, 56, 56, 56, 56)?)
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
		let adj = op.tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
		assert_eq!(adj.average_color(), expected_color);
		Ok(())
	}

	#[tokio::test]
	async fn test_default_values() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// No parameters specified - should use defaults (brightness=0, contrast=1, gamma=1)
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_flatten color=[50,150,250] | raster_levels")
			.await?;

		let bbox = TileCoord::new(3, 2, 1)?.to_tile_bbox();
		let image = op.tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
		// With defaults, output should match the case (0, 1.0, 1.0) above
		assert_eq!(image.average_color(), [63, 157, 249]);
		Ok(())
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_levels")
			.await?;

		let source_type = op.source_type();
		assert!(source_type.to_string().contains("raster_levels"));
		Ok(())
	}

	#[tokio::test]
	async fn test_metadata_and_tilejson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_levels")
			.await?;

		// metadata and tilejson should be passed through from source
		let _metadata = op.metadata();
		let _tilejson = op.tilejson();
		Ok(())
	}

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "raster_levels");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("brightness"));
		assert!(docs.contains("contrast"));
		assert!(docs.contains("gamma"));
	}
}
