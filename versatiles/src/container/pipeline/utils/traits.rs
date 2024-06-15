use crate::container::TilesReaderParameters;
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCoord3, TileStream};

#[async_trait]
pub trait OperationTrait: Debug + Send + Sync + Unpin {
	fn get_parameters(&self) -> &TilesReaderParameters;
	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream;
	fn get_meta(&self) -> Option<Blob>;
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>>;
}

pub trait OperationDocsTrait {
	fn generate_docs() -> String;
}
