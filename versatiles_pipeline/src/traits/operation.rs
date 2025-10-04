use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use versatiles_core::{TileBBox, TileStream, TilesReaderParameters, Traversal, tilejson::*};

use crate::helpers::Tile;

#[async_trait]
pub trait OperationTrait: Debug + Send + Sync + Unpin {
	fn parameters(&self) -> &TilesReaderParameters;
	fn tilejson(&self) -> &TileJSON;
	fn traversal(&self) -> &Traversal {
		&Traversal::ANY
	}
	async fn get_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>>;
}
