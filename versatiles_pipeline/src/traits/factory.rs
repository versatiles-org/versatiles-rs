use crate::{OperationTrait, PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;

pub trait OperationFactoryTrait: Send + Sync {
	fn get_tag_name(&self) -> &str;
	fn get_docs(&self) -> String;
}

#[async_trait]
pub trait ReadOperationFactoryTrait: OperationFactoryTrait {
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn OperationTrait>>;
}

#[async_trait]
pub trait TransformOperationFactoryTrait: OperationFactoryTrait {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn OperationTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>>;
}
