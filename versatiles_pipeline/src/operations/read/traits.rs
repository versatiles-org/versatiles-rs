use crate::{traits::OperationTilesTrait, vpl::VPLNode, OperationTrait, PipelineFactory};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;

pub trait ReadOperationTrait: OperationTrait {
	fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait;
}
