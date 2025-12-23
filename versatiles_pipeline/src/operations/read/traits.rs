use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use versatiles_container::TileSourceTrait;

pub trait ReadTileSourceTrait: TileSourceTrait {
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSourceTrait>>
	where
		Self: Sized + TileSourceTrait;
}
