use anyhow::Result;
use imageproc::image::Rgb;
use versatiles_container::{Tile, TileSource};
use versatiles_core::TileCoord;
use versatiles_derive::context;
use versatiles_image::{color::HexColor, traits::DynamicImageTraitOperation};

use crate::{
	PipelineFactory,
	operations::transform::{TileTransform, TransformOp},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Composites translucent raster tiles onto an opaque background colour.
struct Args {
	/// Background colour, as `RRGGBB` or `[r,g,b]`. Defaults to `FFFFFF`.
	#[vpl(default = "FFFFFF")]
	color: Option<HexColor>,
}

#[derive(Debug)]
struct Operation {
	color: Rgb<u8>,
	/// Whether the tiles hold heights, going by the tile schema of the source.
	elevation: bool,
}

impl Operation {
	#[context("Building raster_flatten operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<Operation>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		// Opaque by construction: compositing onto a translucent background is
		// not a thing this operation can do, so `RRGGBBAA` is refused here
		// rather than having its alpha quietly dropped.
		let color = args.color.map_or(Ok([255, 255, 255]), |color| color.rgb())?;
		let operation = Operation {
			color: Rgb(color),
			elevation: source
				.tilejson()
				.tile_schema
				.is_some_and(|schema| schema.is_elevation()),
		};
		Ok(TransformOp::new(source, operation, factory.runtime()))
	}
}

impl TileTransform for Operation {
	const TAG: &'static str = "raster_flatten";

	fn run(&self, _coord: &TileCoord, mut tile: Tile) -> Result<Option<Tile>> {
		if !tile.as_image()?.has_alpha() {
			return Ok(Some(tile));
		}
		let format = tile.format();
		// The flattened tile is a new tile; see `raster_mask` for why it takes
		// the old one's quality along.
		let quality = tile.format_quality().or(self.elevation.then_some(100));
		let image = tile.into_image()?.into_flattened(self.color)?;
		let mut tile = Tile::from_image(image, format)?;
		tile.set_format_quality(quality);
		Ok(Some(tile))
	}
}

crate::operations::macros::define_transform_factory!("raster_flatten", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use versatiles_core::TileCoord;
	use versatiles_image::DynamicImageTraitOperation;

	use super::*;
	use crate::{PipelineFactory, factory::OperationFactoryTrait};

	#[tokio::test]
	async fn test_raster_flatten_custom_color() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_flatten color=[255,127,0]")
			.await?;

		let bbox = TileCoord::new(2, 1, 1)?.to_tile_bbox();
		let image = op.tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
		assert_eq!(image.average_color(), [238, 119, 0]);

		let bbox = TileCoord::new(2, 2, 1)?.to_tile_bbox();
		let image = op.tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
		assert_eq!(image.average_color(), [254, 135, 16]);

		Ok(())
	}

	/// The two spellings are one colour, which is the compatibility #260 asked
	/// for: `[255,127,0]` is what this operation took before it had a type, and
	/// `FF7F00` is what `from_color` has always taken.
	#[tokio::test]
	async fn both_spellings_of_a_colour_flatten_alike() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let mut flattened = Vec::new();

		for vpl in [
			"from_debug format=png | raster_flatten color=[255,127,0]",
			"from_debug format=png | raster_flatten color=FF7F00",
		] {
			let op = factory.operation_from_vpl(vpl).await?;
			let bbox = TileCoord::new(2, 1, 1)?.to_tile_bbox();
			let image = op.tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
			flattened.push(image.average_color());
		}

		assert_eq!(flattened[0], flattened[1], "the same colour written two ways");
		Ok(())
	}

	/// `HexColor` accepts `RRGGBBAA` because `from_color` has a use for it, but
	/// a background to composite *onto* cannot be translucent — so the refusal
	/// belongs to this operation rather than to the type.
	#[tokio::test]
	async fn a_translucent_background_is_refused() {
		let error = PipelineFactory::new_dummy()
			.operation_from_vpl("from_debug format=png | raster_flatten color=FF7F0080")
			.await
			.expect_err("a background with an alpha channel is not a background");
		assert!(format!("{error:#}").contains("alpha channel"), "{error:#}");
	}

	#[tokio::test]
	async fn test_raster_flatten_default_color() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// No color specified, defaults to white [255, 255, 255]
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_flatten")
			.await?;

		let bbox = TileCoord::new(2, 1, 1)?.to_tile_bbox();
		let image = op.tile_stream(bbox).await?.next().await.unwrap().1.into_image()?;
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
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "raster_flatten");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("color"));
	}

	/// A flattened tile is a new tile, and takes the quality along that the
	/// old one carried; for elevation tiles it is encoded without loss.
	#[rstest::rstest]
	#[case("meta_update schema=\"dem/mapbox\" | ", Some(100))]
	#[case("", None)]
	#[tokio::test]
	async fn a_flattened_elevation_tile_is_encoded_without_loss(
		#[case] schema: &str,
		#[case] expected_quality: Option<u8>,
	) -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_color color=10203080 format=webp tile_size=256 | {schema}raster_flatten"
			))
			.await?;
		let bbox = TileCoord::new(3, 2, 2)?.to_tile_bbox();
		let tile = op.tile_stream(bbox).await?.to_vec().await.remove(0).1;
		assert!(!tile.has_blob(), "flattened, so not encoded yet");
		assert_eq!(tile.format_quality(), expected_quality);
		Ok(())
	}
}
