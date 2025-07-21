use crate::{
	helpers::{pack_vector_tile, pack_vector_tile_stream},
	traits::OperationTrait,
};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use std::sync::Arc;
use versatiles_core::{
	tilejson::TileJSON,
	types::{Blob, TileBBox, TileCompression, TileCoord3, TileStream, TileType, TilesReaderParameters},
};
use versatiles_geometry::vector_tile::VectorTile;

pub trait RunnerTrait: std::fmt::Debug + Send + Sync + 'static {
	fn update_tilejson(&self, tilejson: &mut TileJSON);
	fn run(&self, tile: VectorTile) -> Result<VectorTile>;
}

/// Generic “transform” operation that delegates all real work to a `Runner`.
#[derive(Debug)]
pub struct TransformOp<R: RunnerTrait> {
	pub runner: Arc<R>,
	pub source: Box<dyn OperationTrait>,
	pub params: TilesReaderParameters,
	pub tilejson: TileJSON,
}

#[async_trait]
impl<R: RunnerTrait> OperationTrait for TransformOp<R> {
	/* --- metadata --- */
	fn parameters(&self) -> &TilesReaderParameters {
		&self.params
	}
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/* --- raster requests are invalid for vector transforms --- */
	async fn get_image_data(&self, _: &TileCoord3) -> Result<Option<DynamicImage>> {
		bail!("vector transform cannot return raster data");
	}
	async fn get_image_stream(&self, _: TileBBox) -> Result<TileStream<DynamicImage>> {
		bail!("vector transform cannot return raster data");
	}

	/* --- vector path: run the runner, then pack/unpack as needed --- */
	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		if let Some(tile) = self.source.get_vector_data(coord).await? {
			self.runner.run(tile).map(Some)
		} else {
			Ok(None)
		}
	}
	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		let runner = self.runner.clone();
		Ok(self
			.source
			.get_vector_stream(bbox)
			.await?
			.filter_map_item_parallel(move |tile| runner.run(tile).map(Some)))
	}

	/* --- convenience wrappers to hand out packed blobs --- */
	async fn get_tile_data(&self, c: &TileCoord3) -> Result<Option<Blob>> {
		pack_vector_tile(self.get_vector_data(c).await, &self.params)
	}
	async fn get_tile_stream(&self, b: TileBBox) -> Result<TileStream> {
		pack_vector_tile_stream(self.get_vector_stream(b).await, &self.params)
	}
}

// transform_factory.rs
pub async fn build_transform<R>(source: Box<dyn OperationTrait>, runner: R) -> Result<Box<dyn OperationTrait>>
where
	R: RunnerTrait,
{
	// ── common steps ───────────────────────────────────────────────
	let mut params = source.parameters().clone();
	ensure!(
		params.tile_format.get_type() == TileType::Vector,
		"source must be vector tiles"
	);
	params.tile_compression = TileCompression::Uncompressed;

	// ── runner creation delegated to the caller ────────────────────
	let runner = Arc::new(runner);

	// ── tile-json patching (always the same) ───────────────────────
	let mut tilejson = source.tilejson().clone();
	runner.update_tilejson(&mut tilejson);
	tilejson.update_from_reader_parameters(&params);

	Ok(Box::new(TransformOp::<R> {
		runner,
		source,
		params,
		tilejson,
	}) as Box<dyn OperationTrait>)
}
