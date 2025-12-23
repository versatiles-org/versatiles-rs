use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use versatiles_container::TileSourceTrait;

pub trait OperationFactoryTrait: Send + Sync {
	fn get_tag_name(&self) -> &str;
	fn get_docs(&self) -> String;
}

/// Factory trait for read operations that create tile sources from VPL nodes.
///
/// Read operations are the entry points of a pipeline, creating tile sources
/// from files, databases, or other data sources.
#[async_trait]
pub trait ReadOperationFactoryTrait: OperationFactoryTrait {
	/// Build a tile source from a VPL node configuration.
	///
	/// Returns a boxed [`TileSourceTrait`] (which also implements [`TileSourceTrait`]
	/// via blanket implementation) that can be used as the start of a pipeline.
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn TileSourceTrait>>;
}

/// Factory trait for transform operations that wrap and modify existing tile sources.
///
/// Transform operations take an upstream tile source and apply transformations,
/// filtering, or other processing to the tiles.
#[async_trait]
pub trait TransformOperationFactoryTrait: OperationFactoryTrait {
	/// Build a transform operation that wraps an existing tile source.
	///
	/// Takes a source tile stream and VPL node configuration, returning a new
	/// tile source that applies the transformation.
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn TileSourceTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSourceTrait>>;
}
