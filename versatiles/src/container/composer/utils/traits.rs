use super::Factory;
use crate::{container::TilesReaderParameters, utils::YamlWrapper};
use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::types::{Blob, TileBBox, TileCoord3, TileStream};

#[async_trait]
pub trait OperationTrait: Debug + Send + Sync {
	fn get_docs() -> String
	where
		Self: Sized;
	fn get_id() -> &'static str
	where
		Self: Sized;
	fn get_parameters(&self) -> &TilesReaderParameters;
	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream;
	async fn get_meta(&self) -> Result<Option<Blob>>;
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>>;
}

#[async_trait]
pub trait ReadableOperationTrait: OperationTrait {
	async fn new(yaml: YamlWrapper, builder: &Factory) -> Result<Self>
	where
		Self: Sized;
}

#[async_trait]
pub trait TransformOperationTrait: OperationTrait {
	async fn new(
		yaml: YamlWrapper,
		reader: Box<dyn OperationTrait>,
		builder: &Factory,
	) -> Result<Self>
	where
		Self: Sized;
}
