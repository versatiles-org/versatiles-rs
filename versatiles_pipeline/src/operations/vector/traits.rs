use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::sync::Arc;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TilePyramid, TileStream, TileType};
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
	pub source: Box<dyn TileSource>,
	pub metadata: TileSourceMetadata,
	pub tilejson: TileJSON,
}

#[async_trait]
impl<R: RunnerTrait> TileSource for TransformOp<R> {
	/* --- metadata --- */
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("vector_transform", self.source.source_type())
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self.source.tile_pyramid().await
	}

	#[context("Failed to get transformed tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("vector_transform::tile_stream {bbox:?}");
		let runner = self.runner.clone();
		let tile_format = *self.metadata.tile_format();
		Ok(self
			.source
			.tile_stream(bbox)
			.await?
			.filter_map_parallel_try(move |_coord, tile| {
				let vector = tile.into_vector()?;
				if let Some(transformed_vector) = runner.run(vector)? {
					Ok(Some(Tile::from_vector(transformed_vector, tile_format)?))
				} else {
					Ok(None)
				}
			})
			.unwrap_results())
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.source.tile_coord_stream(bbox).await
	}
}

// transform_factory.rs
#[context("Failed to build transform operation")]
pub async fn build_transform<R>(source: Box<dyn TileSource>, runner: R) -> Result<Box<dyn TileSource>>
where
	R: RunnerTrait,
{
	// ── common steps ───────────────────────────────────────────────
	let metadata = source.metadata().clone();
	ensure!(
		metadata.tile_format().to_type() == TileType::Vector,
		"source must be vector tiles"
	);

	// ── runner creation delegated to the caller ────────────────────
	let runner = Arc::new(runner);

	// ── tile-json patching (always the same) ───────────────────────
	let mut tilejson = source.tilejson().clone();
	runner.update_tilejson(&mut tilejson);
	metadata.update_tilejson(&mut tilejson);

	Ok(Box::new(TransformOp::<R> {
		runner,
		source,
		metadata,
		tilejson,
	}) as Box<dyn TileSource>)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helpers::dummy_vector_source::DummyVectorSource;
	use anyhow::Result;
	use versatiles_core::{TileBBox, TilePyramid};
	use versatiles_geometry::vector_tile::VectorTile;

	#[derive(Debug)]
	struct PassthroughRunner;

	impl RunnerTrait for PassthroughRunner {
		fn update_tilejson(&self, _tilejson: &mut TileJSON) {}
		fn run(&self, tile: VectorTile) -> Result<Option<VectorTile>> {
			Ok(Some(tile))
		}
	}

	#[derive(Debug)]
	struct DropRunner;

	impl RunnerTrait for DropRunner {
		fn update_tilejson(&self, _tilejson: &mut TileJSON) {}
		fn run(&self, _tile: VectorTile) -> Result<Option<VectorTile>> {
			Ok(None)
		}
	}

	fn make_vector_source() -> DummyVectorSource {
		DummyVectorSource::new(
			&[("layer1", &[&[("key", "val")]])],
			Some(TilePyramid::new_full_up_to(2)),
		)
	}

	#[tokio::test]
	async fn test_build_transform_passthrough() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, PassthroughRunner).await?;
		let bbox = TileBBox::new_full(0)?;
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert!(!tiles.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_build_transform_drops_tiles() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, DropRunner).await?;
		let bbox = TileBBox::new_full(0)?;
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert!(tiles.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_build_transform_rejects_raster_source() {
		use crate::helpers::dummy_image_source::DummyImageSource;
		use versatiles_core::TileFormat;
		let source = Box::new(DummyImageSource::from_color(&[128u8, 0, 0], 256, TileFormat::PNG, None).unwrap());
		let result = build_transform(source, PassthroughRunner).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_transform_op_metadata_and_tilejson() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, PassthroughRunner).await?;
		assert!(!op.metadata().tile_pyramid().unwrap().is_empty());
		assert!(op.tilejson().zoom_min().is_some());
		Ok(())
	}

	#[tokio::test]
	async fn test_transform_op_tile_coord_stream() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, PassthroughRunner).await?;
		let bbox = TileBBox::new_full(0)?;
		let coords = op.tile_coord_stream(bbox).await?.to_vec().await;
		assert!(!coords.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn test_transform_op_source_type() -> Result<()> {
		let source = Box::new(make_vector_source());
		let op = build_transform(source, PassthroughRunner).await?;
		assert!(op.source_type().to_string().contains("vector_transform"));
		Ok(())
	}
}
