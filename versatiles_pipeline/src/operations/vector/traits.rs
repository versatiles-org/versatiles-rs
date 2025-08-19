use crate::{helpers::pack_vector_tile_stream, traits::OperationTrait};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use imageproc::image::DynamicImage;
use std::sync::Arc;
use versatiles_core::{
	Traversal,
	tilejson::TileJSON,
	{TileBBox, TileCompression, TileStream, TileType, TilesReaderParameters},
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

	fn traversal(&self) -> &Traversal {
		self.source.traversal()
	}

	async fn get_image_stream(&self, _: TileBBox) -> Result<TileStream<DynamicImage>> {
		bail!("vector transform cannot return raster data");
	}

	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		let runner = self.runner.clone();
		Ok(self
			.source
			.get_vector_stream(bbox)
			.await?
			.filter_map_item_parallel(move |tile| runner.run(tile).map(Some)))
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
