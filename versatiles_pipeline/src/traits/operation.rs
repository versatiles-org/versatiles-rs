use anyhow::Result;
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use std::fmt::Debug;
use versatiles_core::{tilejson::*, types::*};
use versatiles_geometry::vector_tile::VectorTile;

pub trait OperationTrait: OperationBasicsTrait + OperationTilesTrait {}

pub trait OperationBasicsTrait {
	fn get_parameters(&self) -> &TilesReaderParameters;
	fn get_tilejson(&self) -> &TileJSON;
}

#[async_trait]
pub trait OperationTilesTrait: Debug + Send + Sync + Unpin {
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>>;
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>>;
	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>>;
	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>>;
	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>>;
	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>>;
}

/*

#[async_trait]
pub trait OperationTrait: Debug + Send + Sync + Unpin {
	fn get_parameters(&self) -> &TilesReaderParameters {
		todo!()
	}
	fn get_tilejson(&self) -> &TileJSON {
		todo!()
	}
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		todo!()
	}
	async fn get_tile_stream(&self, bbox: TileBBox) -> TileStream<Blob> {
		todo!()
	}
	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		self
			.get_tile_data(coord)
			.await?
			.map(|blob| {
				let parameters = self.get_parameters();
				blob2image(&decompress(blob, &parameters.tile_compression)?, parameters.tile_format)
			})
			.transpose()
	}

	async fn get_image_stream(&self, bbox: TileBBox) -> TileStream<DynamicImage> {
		let parameters = self.get_parameters().clone();
		self.get_tile_stream(bbox).await.map_item_parallel(move |blob| {
			blob2image(&decompress(blob, &parameters.tile_compression)?, parameters.tile_format)
		})
	}

	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		self
			.get_tile_data(coord)
			.await?
			.map(|blob| VectorTile::from_blob(&decompress(blob, &self.get_parameters().tile_compression)?))
			.transpose()
	}

	async fn get_vector_stream(&self, bbox: TileBBox) -> TileStream<VectorTile> {
		let tile_compression = self.get_parameters().tile_compression;
		self.get_tile_stream(bbox).await.filter_map_item_parallel(move |blob| {
			VectorTile::from_blob(&decompress(blob, &tile_compression)?)
				.map(Some)
				.or_else(|_| Ok(None))
		})
	}
}

pub trait ReadOperationTrait: OperationTrait {
	fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait;
}

pub trait VectorOperationTrait: OperationTrait {}

#[async_trait]
impl<T: VectorOperationTrait> OperationTrait for T {
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		Ok(if let Some(vector_data) = self.get_vector_data(coord).await? {
			Some(compress(
				vector_data.to_blob()?,
				&self.get_parameters().tile_compression,
			)?)
		} else {
			None
		})
	}
	async fn get_tile_stream(&self, bbox: TileBBox) -> TileStream<Blob> {
		let tile_compression = self.get_parameters().tile_compression;
		self
			.get_vector_stream(bbox)
			.await
			.map_item_parallel(move |vector_data| compress(vector_data.to_blob()?, &tile_compression))
	}
}

pub trait ImageOperationTrait: OperationTrait {
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let parameters = self.get_parameters();
		Ok(if let Some(image_data) = self.get_image_data(coord).await? {
			Some(compress(
				image2blob(&image_data, parameters.tile_format)?,
				&parameters.tile_compression,
			)?)
		} else {
			None
		})
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> TileStream<Blob> {
		let parameters = self.get_parameters().clone();
		self.get_image_stream(bbox).await.map_item_parallel(move |image_data| {
			compress(
				image2blob(&image_data, parameters.tile_format)?,
				&parameters.tile_compression,
			)
		})
	}
}
 */
