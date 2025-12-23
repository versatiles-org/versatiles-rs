use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::{SourceType, Tile, TileSourceTrait};
use versatiles_core::{TileBBox, TileJSON, TileStream, TileType, TilesReaderParameters, Traversal};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;

pub trait RunnerTrait: std::fmt::Debug + Send + Sync + 'static {
	fn update_tilejson(&self, tilejson: &mut TileJSON);
	fn run(&self, tile: VectorTile) -> Result<Option<VectorTile>>;
}

/// Generic “transform” operation that delegates all real work to a `Runner`.
#[derive(Debug)]
pub struct TransformOp<R: RunnerTrait> {
	pub runner: Arc<R>,
	pub source: Box<dyn TileSourceTrait>,
	pub params: TilesReaderParameters,
	pub tilejson: TileJSON,
}

#[async_trait]
impl<R: RunnerTrait> TileSourceTrait for TransformOp<R> {
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

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("vector_transform", self.source.source_type())
	}

	#[context("Failed to get transformed tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		let runner = self.runner.clone();
		let tile_format = self.params.tile_format;
		Ok(self
			.source
			.get_tile_stream(bbox)
			.await?
			.filter_map_item_parallel(move |tile| {
				let vector = tile.into_vector()?;
				if let Some(transformed_vector) = runner.run(vector)? {
					Ok(Some(Tile::from_vector(transformed_vector, tile_format)?))
				} else {
					Ok(None)
				}
			}))
	}
}

// transform_factory.rs
#[context("Failed to build transform operation")]
pub async fn build_transform<R>(source: Box<dyn TileSourceTrait>, runner: R) -> Result<Box<dyn TileSourceTrait>>
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
	}) as Box<dyn TileSourceTrait>)
}
