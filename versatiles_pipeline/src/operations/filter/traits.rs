use crate::OperationTrait;
use anyhow::Result;
use async_trait::async_trait;
use futures::future::ready;
use imageproc::image::DynamicImage;
use versatiles_core::{
	tilejson::TileJSON,
	types::{Blob, TileBBox, TileCoord3, TileStream, TilesReaderParameters},
};
use versatiles_geometry::vector_tile::VectorTile;

#[async_trait]
pub trait FilterOperationTrait: OperationTrait {
	fn get_prepared_parameters(&self) -> &TilesReaderParameters;
	fn get_prepared_tilejson(&self) -> &TileJSON;
	fn get_source(&self) -> &dyn OperationTrait;
	fn filter_coord(&self, coord: &TileCoord3) -> bool;
}

#[async_trait]
impl<T: FilterOperationTrait> OperationTrait for T {
	#[inline(always)]
	fn get_parameters(&self) -> &TilesReaderParameters {
		self.get_prepared_parameters()
	}

	#[inline(always)]
	fn get_tilejson(&self) -> &TileJSON {
		self.get_prepared_tilejson()
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		match self.filter_coord(coord) {
			true => self.get_source().get_tile_data(coord).await,
			false => Ok(None),
		}
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		Ok(self
			.get_source()
			.get_tile_stream(bbox)
			.await?
			.filter_coord(|coord| ready(self.filter_coord(&coord))))
	}

	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		match self.filter_coord(coord) {
			true => self.get_source().get_image_data(coord).await,
			false => Ok(None),
		}
	}

	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		Ok(self
			.get_source()
			.get_image_stream(bbox)
			.await?
			.filter_coord(|coord| ready(self.filter_coord(&coord))))
	}

	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		match self.filter_coord(coord) {
			true => self.get_source().get_vector_data(coord).await,
			false => Ok(None),
		}
	}

	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		Ok(self
			.get_source()
			.get_vector_stream(bbox)
			.await?
			.filter_coord(|coord| ready(self.filter_coord(&coord))))
	}
}
