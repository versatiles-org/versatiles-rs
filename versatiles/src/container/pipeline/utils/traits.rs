use crate::{container::TilesReaderParameters, utils::vpl::VPLNode};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCoord3, TileStream};

use super::PipelineFactory;

#[async_trait]
pub trait OperationTrait: Debug + Send + Sync + Unpin {
	fn get_parameters(&self) -> &TilesReaderParameters;
	fn get_meta(&self) -> Option<Blob>;
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>>;
	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream;
}

pub trait OperationFactoryTrait: Send + Sync {
	fn get_tag_name(&self) -> &str;
	fn get_docs(&self) -> String;
}

#[async_trait]
pub trait ReadOperationFactoryTrait: OperationFactoryTrait {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>>;
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
