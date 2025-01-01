use crate::{vpl::VPLNode, PipelineFactory};
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::fmt::Debug;
use versatiles_core::{types::*, utils::TileJSON};

#[async_trait]
pub trait OperationTrait: Debug + Send + Sync + Unpin {
	fn get_parameters(&self) -> &TilesReaderParameters;
	fn get_tilejson(&self) -> &TileJSON;
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>>;
	async fn get_tile_stream(&self, bbox: TileBBox) -> TileStream;
}

pub trait ReadOperationTrait: OperationTrait {
	fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait;
}
