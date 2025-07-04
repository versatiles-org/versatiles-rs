use anyhow::Result;
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use std::fmt::Debug;
use versatiles_core::{tilejson::*, types::*};
use versatiles_geometry::vector_tile::VectorTile;

#[async_trait]
pub trait OperationTrait: Debug + Send + Sync + Unpin {
	fn get_parameters(&self) -> &TilesReaderParameters;
	fn get_tilejson(&self) -> &TileJSON;
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>>;
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>>;
	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>>;
	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>>;
	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>>;
	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>>;
}
