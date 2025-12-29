use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use versatiles_container::TileSource;

pub trait ReadTileSource: TileSource {
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource;
}
