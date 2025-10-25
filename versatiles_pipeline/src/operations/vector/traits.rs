use crate::traits::OperationTrait;
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::Tile;
use versatiles_core::{TileBBox, TileJSON, TileStream, TileType, TilesReaderParameters, Traversal};
use versatiles_geometry::vector_tile::VectorTile;

pub trait RunnerTrait: std::fmt::Debug + Send + Sync + 'static {
	fn update_tilejson(&self, tilejson: &mut TileJSON);
	fn run(&self, tile: VectorTile) -> Result<Option<VectorTile>>;
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

	async fn get_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		let runner = self.runner.clone();
		let tile_format = self.params.tile_format;
		Ok(self
			.source
			.get_stream(bbox)
			.await?
			.filter_map_item_parallel(move |tile| {
				let vector = tile.into_vector();
				Ok(runner.run(vector)?.map(|vector| Tile::from_vector(vector, tile_format)))
			}))
	}
}

// transform_factory.rs
pub async fn build_transform<R>(source: Box<dyn OperationTrait>, runner: R) -> Result<Box<dyn OperationTrait>>
where
	R: RunnerTrait,
{
	// ── common steps ───────────────────────────────────────────────
	let params = source.parameters().clone();
	ensure!(
		params.tile_format.to_type() == TileType::Vector,
		"source must be vector tiles"
	);

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
