use anyhow::Result;
use versatiles_container::{RemappedTileSource, TileSource};
use versatiles_core::TileRemap;
use versatiles_derive::context;

use crate::{PipelineFactory, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Relabels tile coordinates, correcting a source that uses TMS row order or `z/y/x` paths.
///
/// The three flags are applied in a fixed order — `flip_x`, then `flip_y`, then
/// `swap_xy` — and between them reach all eight symmetries of the square: four
/// rotations and four reflections, which is every relabelling that maps the tile
/// grid onto itself. Because that set is closed, chaining two of these
/// operations is never necessary; some single combination of the three flags
/// does the same thing.
///
/// The combinations most likely to be wanted:
///
/// | `flip_x` | `flip_y` | `swap_xy` | result                    |
/// | -------- | -------- | --------- | ------------------------- |
/// | false    | true     | false     | TMS ↔ XYZ row order       |
/// | false    | false    | true      | `z/y/x` ↔ `z/x/y` layouts |
/// | true     | true     | false     | rotate 180°               |
/// | true     | false    | true      | rotate 90°                |
///
/// Unlike a global `--flip-y` flag, this applies to one source, so a pipeline
/// can combine sources that disagree about their conventions.
struct Args {
	/// Whether to mirror horizontally, so `x` becomes `2^z - 1 - x`. Defaults to `false`.
	#[vpl(default = "false")]
	flip_x: Option<bool>,
	/// Whether to mirror vertically, so `y` becomes `2^z - 1 - y`. Defaults to `false`.
	#[vpl(default = "false")]
	flip_y: Option<bool>,
	/// Whether to exchange the axes, so `(x, y)` becomes `(y, x)`. Defaults to `false`.
	#[vpl(default = "false")]
	swap_xy: Option<bool>,
}

impl Args {
	fn remap(&self) -> TileRemap {
		TileRemap::new(
			self.flip_x.unwrap_or(false),
			self.flip_y.unwrap_or(false),
			self.swap_xy.unwrap_or(false),
		)
	}
}

/// Thin shell over [`RemappedTileSource`], which does the work and is shared
/// with the converter so the two cannot disagree.
pub struct Operation;

impl Operation {
	#[context("Building remap_coords operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		_factory: &PipelineFactory,
	) -> Result<RemappedTileSource> {
		let args = Args::from_vpl_node(&vpl_node)?;
		RemappedTileSource::new(std::sync::Arc::new(source), args.remap()).await
	}
}

crate::operations::macros::define_transform_factory!("remap_coords", Args, Operation);

#[cfg(test)]
mod tests {
	use versatiles_core::TileCoord;

	use super::*;
	use crate::PipelineFactory;

	#[tokio::test]
	async fn defaults_to_changing_nothing() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let plain = factory.operation_from_vpl("from_debug format=mvt").await?;
		let remapped = factory
			.operation_from_vpl("from_debug format=mvt | remap_coords")
			.await?;

		assert_eq!(
			plain.tile_pyramid().await?.count_tiles(),
			remapped.tile_pyramid().await?.count_tiles()
		);
		Ok(())
	}

	/// The point of the operation: it applies per source, so two sources with
	/// different conventions can be combined in one pipeline — which a global
	/// `--flip-y` cannot express.
	#[tokio::test]
	async fn applies_to_one_source_within_a_pipeline() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_stacked [ from_debug format=mvt | remap_coords flip_y=true, from_debug format=mvt ]")
			.await?;

		assert!(op.tile_pyramid().await?.count_tiles() > 0);
		Ok(())
	}

	/// A flip is its own inverse, so applying it twice returns the original.
	#[tokio::test]
	async fn flipping_twice_restores_the_original() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let coord = TileCoord::new(3, 1, 2)?;

		let plain = factory.operation_from_vpl("from_debug format=mvt").await?;
		let twice = factory
			.operation_from_vpl("from_debug format=mvt | remap_coords flip_y=true | remap_coords flip_y=true")
			.await?;

		let expected = plain.tile(&coord).await?.is_some();
		assert_eq!(twice.tile(&coord).await?.is_some(), expected);
		Ok(())
	}

	/// Relabelling invalidates any order the source promised.
	#[tokio::test]
	async fn the_traversal_is_reset() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=mvt | remap_coords swap_xy=true")
			.await?;
		assert!(op.metadata().traversal().is_any());
		Ok(())
	}

	#[tokio::test]
	async fn rejects_an_unknown_parameter() {
		let factory = PipelineFactory::new_dummy();
		assert!(
			factory
				.operation_from_vpl("from_debug format=mvt | remap_coords flip_z=true")
				.await
				.is_err()
		);
	}
}
