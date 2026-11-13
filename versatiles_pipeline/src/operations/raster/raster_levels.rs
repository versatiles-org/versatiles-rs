use anyhow::Result;
use versatiles_container::{Tile, TileSource};
use versatiles_core::{TileCoord, bounded_number};
use versatiles_derive::context;
use versatiles_image::{DynamicImage, traits::DynamicImageTraitOperation};

use crate::{PipelineFactory, operations::transform::TileTransform, vpl::VPLNode};

bounded_number! {
	/// An offset added to every colour channel, in 0..=255 terms.
	Brightness: f32, -255.0..=255.0;
	/// A factor applied around mid-grey. Zero would flatten every tile to one
	/// colour and a negative factor would invert it, so neither is accepted.
	Contrast: f32, greater_than 0.0;
	/// A gamma exponent. Zero divides by zero rather than being a mild edge.
	Gamma: f32, greater_than 0.0;
	/// A factor applied to each colour's distance from grey. A negative one
	/// would turn colours into their opposites, which is not saturation.
	Saturation: f32, at_least 0.0;
}

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Adjusts the saturation, contrast, brightness and gamma of raster tiles.
///
/// They apply in that order, whichever of them are given, so a set of values
/// always means the same.
///
/// ```vpl
/// from_container filename="satellite.versatiles" | raster_levels saturation=0.65 brightness=65 gamma=1.1
/// ```
///
/// Saturation moves each colour channel towards the pixel's grey value, its
/// Rec. 709 luma `0.2126 r + 0.7152 g + 0.0722 b`, or away from it above `1`.
/// Greyscale images have no colour to change. Alpha is never changed.
struct Args {
	/// Offset added to every channel. Defaults to `0.0`.
	#[vpl(default = "0.0")]
	brightness: Option<Brightness>,
	/// Factor applied around mid-grey. Defaults to `1.0`.
	#[vpl(default = "1.0", step = "0.1")]
	contrast: Option<Contrast>,
	/// Gamma exponent. Defaults to `1.0`.
	#[vpl(default = "1.0", step = "0.1")]
	gamma: Option<Gamma>,
	/// Factor applied to each colour's distance from grey; `0` gives greyscale. Defaults to `1.0`.
	#[vpl(default = "1.0", step = "0.1")]
	saturation: Option<Saturation>,
}

#[derive(Debug)]
struct Operation {
	/// Pre-divided by 255 at build time: `run` is called once per tile.
	brightness: f32,
	contrast: f32,
	gamma: f32,
	saturation: f32,
}

impl Operation {
	#[context("Building raster_levels operation in VPL node {:?}", vpl_node.name)]
	async fn build_transform(
		vpl_node: VPLNode,
		_source: &dyn TileSource,
		_factory: &PipelineFactory,
	) -> Result<Operation> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let operation = Operation {
			brightness: args.brightness.map_or(0.0, Brightness::get) / 255.0,
			contrast: args.contrast.map_or(1.0, Contrast::get) / 255.0,
			gamma: args.gamma.map_or(1.0, Gamma::get),
			saturation: args.saturation.map_or(1.0, Saturation::get),
		};
		Ok(operation)
	}
}

impl TileTransform for Operation {
	const TAG: &'static str = "raster_levels";

	fn run(&self, _coord: &TileCoord, mut tile: Tile) -> Result<Option<Tile>> {
		let (contrast, brightness, gamma) = (self.contrast, self.brightness, self.gamma);
		let image = tile.as_image_mut()?;
		// Exactly `1` changes nothing, so the pass is skipped rather than paid for.
		#[expect(
			clippy::float_cmp,
			reason = "`1` is the default, given or left out, not a computed value"
		)]
		if self.saturation != 1.0 {
			saturate(image, self.saturation);
		}
		#[expect(
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "clamped to 0..=255 before the cast"
		)]
		image.map_color_values(|v| {
			(((f32::from(v) - 127.5) * contrast + 0.5 + brightness).powf(gamma) * 255.0)
				.round()
				.clamp(0.0, 255.0) as u8
		});
		Ok(Some(tile))
	}
}

/// Scales each colour channel's distance from the pixel's Rec. 709 luma by
/// `factor`. Greyscale images, which have no colour, and alpha are left alone.
fn saturate(image: &mut DynamicImage, factor: f32) {
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "clamped to 0..=255 before the cast"
	)]
	let apply = |rgb: &mut [u8]| {
		let [r, g, b] = [rgb[0], rgb[1], rgb[2]].map(f32::from);
		let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
		for c in &mut rgb[..3] {
			*c = (luma + (f32::from(*c) - luma) * factor).round().clamp(0.0, 255.0) as u8;
		}
	};
	match image {
		DynamicImage::ImageRgb8(img) => img.pixels_mut().for_each(|p| apply(&mut p.0)),
		DynamicImage::ImageRgba8(img) => img.pixels_mut().for_each(|p| apply(&mut p.0)),
		_ => {}
	}
}

crate::operations::macros::define_transform_factory!("raster_levels", Args, Operation, requires: Raster, per_tile);

#[cfg(test)]
mod tests {
	use rstest::rstest;
	use versatiles_container::TilesRuntime;
	use versatiles_core::{TileBBox, TileFormat};
	use versatiles_image::DynamicImageTraitOperation;

	use super::*;
	use crate::{
		factory::OperationFactoryTrait, helpers::dummy_image_source::DummyImageSource, operations::transform::TransformOp,
	};

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
				saturation: 1.0,
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

	/// The colour of a flat tile after `raster_levels` with these values.
	async fn levels_of(color: &[u8], saturation: f32, brightness: f32) -> Result<Vec<u8>> {
		let op = TransformOp::new(
			Box::new(DummyImageSource::from_color(color, 4, TileFormat::PNG, None).unwrap()),
			Operation {
				brightness: brightness / 255.0,
				contrast: 1.0 / 255.0,
				gamma: 1.0,
				saturation,
			},
			TilesRuntime::new_silent(),
		);
		let bbox = TileBBox::from_min_and_max(8, 56, 56, 56, 56)?;
		let mut tiles = op.tile_stream(bbox).await?.to_vec().await;
		Ok(tiles[0].1.as_image()?.average_color())
	}

	// Luma of [51, 119, 170] is 108.2.
	#[rstest]
	#[case::unchanged(1.0, [51, 119, 170])]
	#[case::greyscale(0.0, [108, 108, 108])]
	#[case::halfway_to_grey(0.5, [80, 114, 139])]
	#[case::boosted_and_clamped(2.0, [0, 130, 232])]
	#[tokio::test]
	async fn saturation_scales_the_distance_from_grey(#[case] saturation: f32, #[case] expected: [u8; 3]) -> Result<()> {
		assert_eq!(levels_of(&[51, 119, 170], saturation, 0.0).await?, expected);
		Ok(())
	}

	#[test]
	fn saturation_leaves_greyscale_and_alpha_alone() {
		let mut grey = DynamicImage::ImageLumaA8(versatiles_image::ImageBuffer::from_pixel(2, 2, [90, 200].into()));
		saturate(&mut grey, 0.0);
		assert_eq!(grey.average_color(), [90, 200]);

		let mut rgba = DynamicImage::ImageRgba8(versatiles_image::ImageBuffer::from_pixel(
			2,
			2,
			[51, 119, 170, 77].into(),
		));
		saturate(&mut rgba, 0.0);
		assert_eq!(rgba.average_color(), [108, 108, 108, 77]);
	}

	/// Saturation comes first: greyed at 108, then brightened to 208. The other
	/// way round, the brightened colour [151, 219, 255] would grey to 207.
	#[tokio::test]
	async fn saturation_applies_before_brightness() -> Result<()> {
		assert_eq!(levels_of(&[51, 119, 170], 0.0, 100.0).await?, [208, 208, 208]);
		Ok(())
	}

	#[tokio::test]
	async fn a_negative_saturation_is_refused() {
		let error = PipelineFactory::new_dummy()
			.operation_from_vpl("from_debug format=png | raster_levels saturation=-0.5")
			.await
			.unwrap_err();
		assert!(format!("{error:#}").contains("must be at least 0"), "{error:#}");
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
