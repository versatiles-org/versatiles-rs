use crate::{OperationTrait, PipelineFactory, vpl::VPLNode};
use anyhow::Result;

pub trait ReadOperationTrait: OperationTrait {
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn OperationTrait>>
	where
		Self: Sized + OperationTrait;
}
