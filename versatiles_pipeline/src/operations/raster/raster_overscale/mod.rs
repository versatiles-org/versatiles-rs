//! # Raster Overscale Operation
//!
//! Creates the raster tiles a source lacks from the tiles it has at lower zoom
//! levels, by cutting out the area of the missing tile and scaling it up.
//!
//! ## How it works
//!
//! A tile the source has is passed on untouched. For a tile it lacks, the
//! operation looks for the nearest tile below it: the parent, then the
//! grandparent, and so on down to the source's lowest level. A z15 tile covers
//! a quarter of its z14 parent, so that quarter is cut out and scaled up to a
//! full tile.
//!
//! This is what makes a sparse source usable: worldwide imagery to z12, say,
//! with aerial imagery to z19 for some regions. Every tile up to `level_max`
//! then has an answer, from the best data there is at that place.
//!
//! ## Where tiles are created
//!
//! - Inside the bounds of the source, as its metadata declares them.
//! - Where the tile below has content: a tile that would be completely
//!   transparent is not created ([`mask`]).
//! - Up to `level_max`, and never at or below the source's lowest level.
//!
//! ## Modules
//!
//! - [`fill`]: the operation behind `raster_overscale` in a pipeline.
//! - [`fixed`]: overscaling from one fixed level, which `from_stacked_raster`
//!   wraps its sources in.
//! - [`mask`]: where a tile has content.
//! - [`scale`]: cutting out and scaling up.

mod fill;
mod fixed;
mod mask;
mod scale;

use anyhow::{Result, bail};
use versatiles_container::TileSource;
use versatiles_core::ZoomLevel;
use versatiles_derive::context;

pub use self::{fill::FillOverscale, fixed::FixedOverscale};
use crate::{PipelineFactory, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Creates the raster tiles a source lacks by upscaling the tiles it has.
///
/// A tile the source has is passed on untouched. A tile it lacks is created
/// from the nearest tile it has at a lower zoom level: the area is cut out and
/// scaled up. The result is blurry rather than detailed — the point is that
/// every tile up to `level_max` has an answer, from the best data there is at
/// that place. For a source with worldwide imagery at its low levels and
/// aerial imagery for some regions on top, that fills in everything between.
///
/// Tiles are only created inside the bounds of the source, so set them with
/// `meta_update bounds=…` in front if they are missing or too wide. A tile that
/// would be completely transparent is not created; a transparent tile the
/// source has is passed on like any other. Nothing is created below the
/// source's lowest level.
///
/// A source whose bounds are the whole world is filled for the whole world,
/// which is fine for serving and far too many tiles to write to a file at a
/// high `level_max`. To set the format, quality or effort of the tiles
/// created, put `raster_format` behind this operation.
pub struct Args {
	/// Highest zoom level to create tiles at. Defaults to the source's highest.
	pub level_max: Option<ZoomLevel>,
}

/// Parameters `raster_overscale` had before it filled gaps (#284), and what to
/// do in their place.
const REMOVED_PARAMETERS: [(&str, &str); 2] = [
	(
		"level_base",
		"tiles are created from the nearest lower zoom level the source has; to ignore its higher levels, put `filter level_max=…` in front",
	),
	(
		"enable_climbing",
		"tiles are always created from the nearest lower zoom level the source has; remove the parameter",
	),
];

/// Builds the operation behind `raster_overscale` in a pipeline.
pub struct Operation;

impl Operation {
	#[context("Building raster_overscale operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<FillOverscale> {
		// A parameter that is gone says so, and says what to do. The generic
		// "does not have a parameter" would leave a pipeline that worked
		// yesterday with a list of names and no way forward.
		for (name, advice) in REMOVED_PARAMETERS {
			if vpl_node.properties.contains_key(name) {
				bail!("'raster_overscale' no longer has the parameter '{name}': {advice}");
			}
		}
		let args = Args::from_vpl_node(&vpl_node)?;
		FillOverscale::new(source, args.level_max.map(u8::from)).await
	}
}

crate::operations::macros::define_transform_factory!("raster_overscale", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use versatiles_core::{TileCoord, TileFormat};

	use super::*;
	use crate::factory::OperationFactoryTrait;

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "raster_overscale");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("level_max"));
		assert!(!docs.contains("level_base"));
		assert!(!docs.contains("enable_climbing"));
	}

	/// `from_debug` has tiles at every level up to 30 and no gaps, so there is
	/// nothing to create: the operation passes its tiles on.
	#[tokio::test]
	async fn in_a_pipeline_it_passes_existing_tiles_on() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_overscale")
			.await?;
		assert_eq!(*op.metadata().tile_format(), TileFormat::PNG);

		let bbox = TileCoord::new(3, 2, 2)?.to_tile_bbox();
		assert_eq!(op.tile_stream(bbox).await?.to_vec().await.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn in_a_pipeline_it_creates_tiles_above_the_source() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | filter level_max=3 | raster_overscale level_max=5")
			.await?;
		assert_eq!(op.metadata().tile_pyramid().unwrap().level_max(), Some(5));

		let bbox = TileCoord::new(5, 9, 9)?.to_tile_bbox();
		assert_eq!(op.tile_stream(bbox).await?.to_vec().await.len(), 1);
		let bbox = TileCoord::new(6, 9, 9)?.to_tile_bbox();
		assert_eq!(op.tile_stream(bbox).await?.to_vec().await.len(), 0);
		Ok(())
	}

	#[rstest::rstest]
	#[case("level_base=2", "filter level_max")]
	#[case("enable_climbing=true", "remove the parameter")]
	#[tokio::test]
	async fn a_removed_parameter_says_what_to_do_instead(#[case] parameter: &str, #[case] advice: &str) {
		let factory = PipelineFactory::new_dummy();
		let error = factory
			.operation_from_vpl(&format!("from_debug format=png | raster_overscale {parameter}"))
			.await
			.unwrap_err();
		let message = format!("{error:#}");
		assert!(message.contains("no longer has the parameter"), "{message}");
		assert!(message.contains(advice), "{message}");
	}
}
